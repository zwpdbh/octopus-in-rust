//! On-disk storage for the gamedata mirror: uploaded files plus manifests,
//! organized into sync channels (gamedata, map-generator).
//!
//! Layout below the configured root:
//!
//! ```text
//! <root>/channels/<channel>/
//!   manifest.json   # generated atomically on commit, never hand-edited
//!   files/<path>    # content served to sync clients
//!   incoming/       # temp dir for in-progress uploads (renamed into files/)
//! ```

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use axum::http::HeaderMap;
use chrono::Utc;
use fafcn_gamedata::{
    compare_version_strings, map_folder_version, sha256_bytes, sha256_file, validate_relative_path,
    FileEntry, Manifest, UploadCommitRequest, CHANNELS, CHANNEL_FAF_CLIENT, CHANNEL_MAP_GENERATOR,
};

use crate::error::{Error, Result};

/// Owns the gamedata mirror directory and the upload credential.
#[derive(Debug)]
pub struct GamedataStore {
    root: PathBuf,
    upload_token: Option<String>,
}

impl GamedataStore {
    /// Create the store, ensuring the per-channel directory layout exists.
    pub fn new(root: PathBuf, upload_token: Option<String>) -> Result<Self> {
        for channel in CHANNELS {
            fs::create_dir_all(root.join("channels").join(channel).join("files"))?;
            fs::create_dir_all(root.join("channels").join(channel).join("incoming"))?;
        }
        Ok(Self { root, upload_token })
    }

    /// Directory served under `/api/gamedata/channels/<channel>/files`.
    pub fn files_dir(&self, channel: &str) -> PathBuf {
        self.channel_root(channel).join("files")
    }

    fn channel_root(&self, channel: &str) -> PathBuf {
        self.root.join("channels").join(channel)
    }

    /// Temp dir for in-progress uploads/downloads (`incoming/`), inside the
    /// channel root so renames into `files/` stay on the same filesystem.
    pub fn incoming_dir(&self, channel: &str) -> PathBuf {
        self.channel_root(channel).join("incoming")
    }

    fn manifest_path(&self, channel: &str) -> PathBuf {
        self.channel_root(channel).join("manifest.json")
    }

    /// Verify the `Authorization: Bearer <token>` header against the
    /// configured upload token.
    pub fn authorize(&self, headers: &HeaderMap) -> Result<()> {
        let Some(expected) = &self.upload_token else {
            return Err(Error::Unavailable(
                "gamedata upload is disabled (FAFCN_GAMEDATA_UPLOAD_TOKEN not set)".to_string(),
            ));
        };
        let presented = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match presented {
            Some(token) if token == expected => Ok(()),
            _ => Err(Error::Unauthorized),
        }
    }

    /// Read a channel's manifest, or `None` if nothing was ever committed.
    pub fn read_manifest(&self, channel: &str) -> Result<Option<Manifest>> {
        let path = self.manifest_path(channel);
        if !path.is_file() {
            return Ok(None);
        }
        let json = fs::read_to_string(&path)?;
        let manifest = serde_json::from_str(&json)
            .map_err(|e| Error::Internal(format!("corrupt manifest.json in {channel}: {e}")))?;
        Ok(Some(manifest))
    }

    /// Return the subset of `files` the server does not already have stored
    /// with a matching hash.
    pub fn check_needed(&self, channel: &str, files: &[FileEntry]) -> Result<Vec<String>> {
        let mut needed = Vec::new();
        for entry in files {
            validate_relative_path(&entry.path).map_err(|e| Error::BadRequest(e.to_string()))?;
            if !self.stored_file_matches(channel, entry)? {
                needed.push(entry.path.clone());
            }
        }
        Ok(needed)
    }

    /// Store one uploaded file: hash-verify, then atomically move from
    /// `incoming/` into `files/`.
    pub fn store_upload(
        &self,
        channel: &str,
        rel_path: &str,
        expected_sha256: &str,
        bytes: &[u8],
    ) -> Result<()> {
        validate_relative_path(rel_path).map_err(|e| Error::BadRequest(e.to_string()))?;
        let actual = sha256_bytes(bytes);
        if actual != expected_sha256 {
            return Err(Error::BadRequest(format!(
                "sha256 mismatch for {rel_path}: expected {expected_sha256}, got {actual}"
            )));
        }

        let tmp = self
            .incoming_dir(channel)
            .join(format!("{}.part", uuid::Uuid::new_v4()));
        fs::write(&tmp, bytes)?;

        let dest = self.files_dir(channel).join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&tmp, &dest)?;
        Ok(())
    }

    /// Store one file already on local disk (e.g. downloaded by the
    /// auto-updater into `incoming/`): streaming-hash `src`, verify it
    /// against `expected_sha256`, then atomically rename it into `files/`.
    /// Same validation as [`Self::store_upload`] but never buffers the file
    /// in memory. On success `src` no longer exists (it was renamed).
    pub fn store_file_from_path(
        &self,
        channel: &str,
        rel_path: &str,
        expected_sha256: &str,
        src: &Path,
    ) -> Result<()> {
        validate_relative_path(rel_path).map_err(|e| Error::BadRequest(e.to_string()))?;
        let actual = sha256_file(src)?;
        if actual != expected_sha256 {
            return Err(Error::BadRequest(format!(
                "sha256 mismatch for {rel_path}: expected {expected_sha256}, got {actual}"
            )));
        }

        let dest = self.files_dir(channel).join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(src, &dest)?;
        Ok(())
    }

    /// Finalize an upload session: verify every listed file is present with a
    /// matching hash, then atomically replace the channel's `manifest.json`.
    ///
    /// Channels with versioned file names (map-generator, faf-client) prune
    /// stored files that are no longer in the committed manifest, so old
    /// releases do not accumulate on disk. The gamedata channel never prunes:
    /// its manifest always covers the same fixed file set, and the maps
    /// channel has its own merge-time pruning in [`Self::commit_merge`].
    pub fn commit(&self, channel: &str, req: &UploadCommitRequest) -> Result<Manifest> {
        self.validate_commit_request(channel, req)?;
        self.guard_against_downgrade(channel, &req.patch_version)?;
        let manifest = Manifest {
            patch_version: req.patch_version.clone(),
            uploader: req.uploader.clone(),
            generated_at: Utc::now(),
            files: req.files.clone(),
        };
        self.write_manifest(channel, &manifest)?;
        if matches!(channel, CHANNEL_MAP_GENERATOR | CHANNEL_FAF_CLIENT) {
            let pruned = self.prune_unlisted_files(channel, &manifest)?;
            if pruned > 0 {
                tracing::info!(channel, pruned, "pruned files no longer in the manifest");
            }
        }
        Ok(manifest)
    }

    /// Finalize a maps upload by MERGING into the existing manifest instead
    /// of replacing it (maps come from many uploaders over time). Every map
    /// whose base name appears in the incoming set has ALL its previously
    /// stored versions replaced by the incoming one; unrelated maps are kept
    /// untouched. Files of replaced map versions are removed from disk.
    ///
    /// No downgrade guard: the maps version is a date stamp, not a patch
    /// number.
    pub fn commit_merge(&self, channel: &str, req: &UploadCommitRequest) -> Result<Manifest> {
        self.validate_commit_request(channel, req)?;

        // Base names (without `.vNNNN`) of the maps being uploaded.
        let incoming_bases: HashSet<String> = req
            .files
            .iter()
            .filter_map(|e| map_folder_version(top_folder(&e.path)))
            .map(|(base, _)| base.to_string())
            .collect();
        let incoming_paths: HashSet<&str> = req.files.iter().map(|e| e.path.as_str()).collect();

        let mut merged: Vec<FileEntry> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        if let Some(existing) = self.read_manifest(channel)? {
            for entry in existing.files {
                let replaced = map_folder_version(top_folder(&entry.path))
                    .is_some_and(|(base, _)| incoming_bases.contains(base));
                if replaced {
                    dropped.push(entry.path);
                } else if !incoming_paths.contains(entry.path.as_str()) {
                    merged.push(entry);
                }
                // Same path in both: incoming entry wins (dedupe).
            }
        }
        merged.extend(req.files.iter().cloned());
        merged.sort_by(|a, b| a.path.cmp(&b.path));

        // Collapse to the newest version per map base: merged uploads (or an
        // uploader whose folder contained several versions of a map) must not
        // leave multiple versions of the same map in the manifest — clients
        // prune their older copies and would otherwise re-download them.
        let mut newest: HashMap<String, u32> = HashMap::new();
        for entry in &merged {
            if let Some((base, version)) = map_folder_version(top_folder(&entry.path)) {
                newest
                    .entry(base.to_string())
                    .and_modify(|v| *v = (*v).max(version))
                    .or_insert(version);
            }
        }
        let mut collapsed = Vec::with_capacity(merged.len());
        for entry in merged {
            let keep = match map_folder_version(top_folder(&entry.path)) {
                Some((base, version)) => newest.get(base) == Some(&version),
                None => true,
            };
            if keep {
                collapsed.push(entry);
            } else {
                dropped.push(entry.path);
            }
        }
        let merged = collapsed;

        // Remove replaced files from disk, then the now-empty old folders.
        let merged_tops: HashSet<&str> = merged.iter().map(|e| top_folder(&e.path)).collect();
        let mut dropped_tops: HashSet<&str> = HashSet::new();
        for path in &dropped {
            dropped_tops.insert(top_folder(path));
            let _ = fs::remove_file(self.files_dir(channel).join(path));
        }
        for top in dropped_tops {
            if !merged_tops.contains(top) {
                let _ = fs::remove_dir_all(self.files_dir(channel).join(top));
            }
        }

        let manifest = Manifest {
            patch_version: req.patch_version.clone(),
            uploader: req.uploader.clone(),
            generated_at: Utc::now(),
            files: merged,
        };
        self.write_manifest(channel, &manifest)?;
        Ok(manifest)
    }

    /// Shared commit validation: non-empty metadata and every listed file
    /// stored with a matching hash.
    fn validate_commit_request(&self, channel: &str, req: &UploadCommitRequest) -> Result<()> {
        if req.patch_version.trim().is_empty() {
            return Err(Error::BadRequest(
                "patch_version must not be empty".to_string(),
            ));
        }
        if req.uploader.trim().is_empty() {
            return Err(Error::BadRequest("uploader must not be empty".to_string()));
        }
        if req.files.is_empty() {
            return Err(Error::BadRequest("file list must not be empty".to_string()));
        }
        for entry in &req.files {
            validate_relative_path(&entry.path).map_err(|e| Error::BadRequest(e.to_string()))?;
            if !self.stored_file_matches(channel, entry)? {
                return Err(Error::BadRequest(format!(
                    "cannot commit: {} is missing or does not match its sha256",
                    entry.path
                )));
            }
        }
        Ok(())
    }

    /// Atomically replace the channel's `manifest.json`.
    fn write_manifest(&self, channel: &str, manifest: &Manifest) -> Result<()> {
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|e| Error::Internal(format!("failed to serialize manifest: {e}")))?;
        let tmp = self.channel_root(channel).join("manifest.json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, self.manifest_path(channel))?;
        Ok(())
    }

    /// Refuse to publish a patch that is strictly older than the one already
    /// on the server (skipped for non-numeric versions).
    fn guard_against_downgrade(&self, channel: &str, new_version: &str) -> Result<()> {
        let Some(existing) = self.read_manifest(channel)? else {
            return Ok(());
        };
        if compare_version_strings(&existing.patch_version, new_version)
            == Some(std::cmp::Ordering::Greater)
        {
            return Err(Error::Conflict(format!(
                "server already has newer {} {} (yours is {new_version}); nothing to upload",
                channel, existing.patch_version
            )));
        }
        Ok(())
    }

    /// Delete every file below `files/` that is not listed in `manifest`,
    /// then remove emptied directories. Returns the number of files removed.
    ///
    /// Only safe for channels whose manifest is the sole source of truth
    /// (map-generator, faf-client) — never call this for gamedata.
    fn prune_unlisted_files(&self, channel: &str, manifest: &Manifest) -> Result<usize> {
        let keep: HashSet<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        let files_dir = self.files_dir(channel);
        let mut removed = 0usize;
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in walkdir::WalkDir::new(&files_dir).contents_first(false) {
            let entry =
                entry.map_err(|e| Error::Internal(format!("failed to walk files dir: {e}")))?;
            let path = entry.path();
            if entry.file_type().is_dir() {
                if path != files_dir {
                    dirs.push(path.to_path_buf());
                }
                continue;
            }
            let rel = relative_slash_path(&files_dir, path).ok_or_else(|| {
                Error::Internal(format!("failed to relativize {}", path.display()))
            })?;
            if !keep.contains(rel.as_str()) {
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        // Deepest first so parents become empty before their turn.
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for dir in dirs {
            let _ = fs::remove_dir(dir); // fails harmlessly when not empty
        }
        Ok(removed)
    }

    /// True when `files/<path>` exists with matching size and hash.
    fn stored_file_matches(&self, channel: &str, entry: &FileEntry) -> Result<bool> {
        let path = self.files_dir(channel).join(&entry.path);
        if !path.is_file() {
            return Ok(false);
        }
        if fs::metadata(&path)?.len() != entry.size {
            return Ok(false);
        }
        Ok(sha256_file(&path)? == entry.sha256)
    }
}

/// Top-level folder of a manifest path (`map.v0001/x.lua` → `map.v0001`;
/// a top-level file maps to itself).
fn top_folder(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// Forward-slash relative path of `path` below `base`, matching the manifest
/// path format (`sub/dir/file.ext`). `None` when `path` is not below `base`.
fn relative_slash_path(base: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(base).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        parts.push(component.as_os_str().to_str()?.to_string());
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fafcn_gamedata::{CHANNEL_COOP, CHANNEL_GAMEDATA, CHANNEL_MAPS};

    fn temp_store() -> (PathBuf, GamedataStore) {
        let root = std::env::temp_dir().join(format!("fafcn-store-test-{}", uuid::Uuid::new_v4()));
        let store = GamedataStore::new(root.clone(), Some("secret".to_string())).unwrap();
        (root, store)
    }

    fn entry_for(bytes: &[u8]) -> FileEntry {
        FileEntry {
            path: "faf.scd".to_string(),
            size: bytes.len() as u64,
            sha256: sha256_bytes(bytes),
        }
    }

    #[test]
    fn upload_check_commit_roundtrip() {
        let (root, store) = temp_store();
        let bytes = b"patch-bytes";
        let entry = entry_for(bytes);

        assert_eq!(
            store
                .check_needed(CHANNEL_GAMEDATA, std::slice::from_ref(&entry))
                .unwrap(),
            vec!["faf.scd"]
        );
        assert!(store
            .store_upload(CHANNEL_GAMEDATA, "faf.scd", "00".repeat(32).as_str(), bytes)
            .is_err());

        store
            .store_upload(CHANNEL_GAMEDATA, "faf.scd", &entry.sha256, bytes)
            .unwrap();
        assert!(store
            .check_needed(CHANNEL_GAMEDATA, std::slice::from_ref(&entry))
            .unwrap()
            .is_empty());

        let manifest = store
            .commit(
                CHANNEL_GAMEDATA,
                &UploadCommitRequest {
                    patch_version: "3825".to_string(),
                    uploader: "tester".to_string(),
                    files: vec![entry],
                },
            )
            .unwrap();
        assert_eq!(manifest.patch_version, "3825");

        let loaded = store.read_manifest(CHANNEL_GAMEDATA).unwrap().unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files[0].sha256, manifest.files[0].sha256);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commit_rejects_missing_files() {
        let (root, store) = temp_store();
        let result = store.commit(
            CHANNEL_GAMEDATA,
            &UploadCommitRequest {
                patch_version: "3825".to_string(),
                uploader: "tester".to_string(),
                files: vec![entry_for(b"nope")],
            },
        );
        assert!(result.is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commit_rejects_older_patch_version() {
        let (root, store) = temp_store();
        let bytes = b"patch-bytes";
        let entry = entry_for(bytes);
        store
            .store_upload(CHANNEL_GAMEDATA, "faf.scd", &entry.sha256, bytes)
            .unwrap();
        let commit = |version: &str| {
            store.commit(
                CHANNEL_GAMEDATA,
                &UploadCommitRequest {
                    patch_version: version.to_string(),
                    uploader: "tester".to_string(),
                    files: vec![entry.clone()],
                },
            )
        };

        commit("3837").unwrap();
        assert!(commit("3837").is_ok());
        let downgraded = commit("3825");
        assert!(matches!(downgraded, Err(Error::Conflict(_))));
        assert!(commit("3900").is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    fn map_entry(path: &str, bytes: &[u8]) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            size: bytes.len() as u64,
            sha256: sha256_bytes(bytes),
        }
    }

    fn store_map_file(store: &GamedataStore, entry: &FileEntry, bytes: &[u8]) {
        store
            .store_upload(CHANNEL_MAPS, &entry.path, &entry.sha256, bytes)
            .unwrap();
    }

    fn merge_req(entries: Vec<FileEntry>) -> UploadCommitRequest {
        UploadCommitRequest {
            patch_version: "2026-08-20".to_string(),
            uploader: "tester".to_string(),
            files: entries,
        }
    }

    #[test]
    fn merge_replaces_old_map_version_and_keeps_others() {
        let (root, store) = temp_store();
        // Existing manifest: map_a v1 + map_b v1.
        let a1 = map_entry("map_a.v0001/a.lua", b"a1");
        let b1 = map_entry("map_b.v0001/b.lua", b"b1");
        store_map_file(&store, &a1, b"a1");
        store_map_file(&store, &b1, b"b1");
        store
            .commit_merge(CHANNEL_MAPS, &merge_req(vec![a1.clone(), b1.clone()]))
            .unwrap();

        // Upload map_a v2: v1 must be replaced, map_b untouched.
        let a2 = map_entry("map_a.v0002/a.lua", b"a2");
        store_map_file(&store, &a2, b"a2");
        let manifest = store
            .commit_merge(CHANNEL_MAPS, &merge_req(vec![a2.clone()]))
            .unwrap();

        let paths: Vec<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["map_a.v0002/a.lua", "map_b.v0001/b.lua"]);
        // Old version files are gone from disk, the rest remains.
        assert!(!root.join("channels/maps/files/map_a.v0001/a.lua").exists());
        assert!(root.join("channels/maps/files/map_a.v0002/a.lua").is_file());
        assert!(root.join("channels/maps/files/map_b.v0001/b.lua").is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn merge_rejects_missing_files() {
        let (root, store) = temp_store();
        let result =
            store.commit_merge(CHANNEL_MAPS, &merge_req(vec![map_entry("m.v0001/x", b"x")]));
        assert!(result.is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn merge_collapses_multiple_versions_to_newest() {
        let (root, store) = temp_store();
        // One upload carrying BOTH versions of a map (uploader's folder had
        // a stale copy): only the newest may survive in the manifest.
        let a1 = map_entry("map_a.v0001/a.lua", b"a1");
        let a2 = map_entry("map_a.v0002/a.lua", b"a2");
        store_map_file(&store, &a1, b"a1");
        store_map_file(&store, &a2, b"a2");
        let manifest = store
            .commit_merge(CHANNEL_MAPS, &merge_req(vec![a1, a2]))
            .unwrap();

        let paths: Vec<&str> = manifest.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["map_a.v0002/a.lua"]);
        assert!(!root.join("channels/maps/files/map_a.v0001").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn store_file_from_path_stores_matching_file() {
        let (root, store) = temp_store();
        let bytes = b"auto-downloaded-patch";
        let src = store
            .incoming_dir(CHANNEL_GAMEDATA)
            .join(format!("auto-{}.part", uuid::Uuid::new_v4()));
        fs::write(&src, bytes).unwrap();

        store
            .store_file_from_path(CHANNEL_GAMEDATA, "env.nx2", &sha256_bytes(bytes), &src)
            .unwrap();

        let stored = root.join("channels/gamedata/files/env.nx2");
        assert_eq!(fs::read(&stored).unwrap(), bytes);
        assert!(!src.exists(), "src was renamed into files/");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn store_file_from_path_rejects_hash_mismatch() {
        let (root, store) = temp_store();
        let src = store
            .incoming_dir(CHANNEL_GAMEDATA)
            .join(format!("auto-{}.part", uuid::Uuid::new_v4()));
        fs::write(&src, b"corrupt").unwrap();

        let result =
            store.store_file_from_path(CHANNEL_GAMEDATA, "env.nx2", &"00".repeat(32), &src);
        assert!(matches!(result, Err(Error::BadRequest(_))));
        assert!(
            !root.join("channels/gamedata/files/env.nx2").exists(),
            "nothing stored on mismatch"
        );
        assert!(src.is_file(), "src left in place for caller cleanup");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn store_file_from_path_rejects_path_traversal() {
        let (root, store) = temp_store();
        let src = root.join("evil.part");
        fs::write(&src, b"x").unwrap();
        assert!(store
            .store_file_from_path(CHANNEL_GAMEDATA, "../evil.scd", &sha256_bytes(b"x"), &src)
            .is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commit_prunes_unlisted_files_in_versioned_channels() {
        let (root, store) = temp_store();
        // map-generator: old jar drops out of the manifest → deleted from disk.
        let v1 = map_entry("MapGenerator_1.22.9.jar", b"v1");
        store
            .store_upload(CHANNEL_MAP_GENERATOR, &v1.path, &v1.sha256, b"v1")
            .unwrap();
        store
            .commit(
                CHANNEL_MAP_GENERATOR,
                &UploadCommitRequest {
                    patch_version: "1.22.9".to_string(),
                    uploader: "tester".to_string(),
                    files: vec![v1],
                },
            )
            .unwrap();
        assert!(root
            .join("channels/map-generator/files/MapGenerator_1.22.9.jar")
            .is_file());

        let v2 = map_entry("MapGenerator_1.22.10.jar", b"v2");
        store
            .store_upload(CHANNEL_MAP_GENERATOR, &v2.path, &v2.sha256, b"v2")
            .unwrap();
        store
            .commit(
                CHANNEL_MAP_GENERATOR,
                &UploadCommitRequest {
                    patch_version: "1.22.10".to_string(),
                    uploader: "tester".to_string(),
                    files: vec![v2],
                },
            )
            .unwrap();

        assert!(
            !root
                .join("channels/map-generator/files/MapGenerator_1.22.9.jar")
                .exists(),
            "old jar pruned from disk"
        );
        assert!(root
            .join("channels/map-generator/files/MapGenerator_1.22.10.jar")
            .is_file());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commit_never_prunes_gamedata_channel() {
        let (root, store) = temp_store();
        let listed = map_entry("env.nx2", b"listed");
        store
            .store_upload(CHANNEL_GAMEDATA, &listed.path, &listed.sha256, b"listed")
            .unwrap();
        // An unlisted file present on disk (e.g. left by an interrupted
        // process) must survive a gamedata commit untouched.
        let extra = root.join("channels/gamedata/files/stray.nx2");
        fs::write(&extra, b"stray").unwrap();

        store
            .commit(
                CHANNEL_GAMEDATA,
                &UploadCommitRequest {
                    patch_version: "3838".to_string(),
                    uploader: "tester".to_string(),
                    files: vec![listed],
                },
            )
            .unwrap();

        assert!(extra.is_file(), "gamedata channel never prunes");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn coop_channel_roundtrip_with_nested_paths() {
        let (root, store) = temp_store();
        // Coop manifest paths carry their bin//gamedata prefixes — nested
        // relative paths must pass validation and store correctly.
        let files = [
            ("bin/init_coop.lua", b"init" as &[u8]),
            ("gamedata/A01_VO.nx2", b"voice-over"),
            ("gamedata/mods.nx2", b"coop-mods"),
        ];
        let mut entries = Vec::new();
        for (path, bytes) in files {
            let entry = FileEntry {
                path: path.to_string(),
                size: bytes.len() as u64,
                sha256: sha256_bytes(bytes),
            };
            store
                .store_upload(CHANNEL_COOP, path, &entry.sha256, bytes)
                .unwrap();
            entries.push(entry);
        }
        let manifest = store
            .commit(
                CHANNEL_COOP,
                &UploadCommitRequest {
                    patch_version: "66".to_string(),
                    uploader: "tester".to_string(),
                    files: entries.clone(),
                },
            )
            .unwrap();
        assert_eq!(manifest.files.len(), 3);
        for entry in &entries {
            assert_eq!(
                fs::read(store.files_dir(CHANNEL_COOP).join(&entry.path)).unwrap(),
                // contents land byte-identical under files/<path>
                match entry.path.as_str() {
                    "bin/init_coop.lua" => b"init".to_vec(),
                    "gamedata/A01_VO.nx2" => b"voice-over".to_vec(),
                    _ => b"coop-mods".to_vec(),
                }
            );
        }
        let loaded = store.read_manifest(CHANNEL_COOP).unwrap().unwrap();
        assert_eq!(loaded.patch_version, "66");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_path_traversal() {
        let (root, store) = temp_store();
        assert!(store
            .store_upload(CHANNEL_GAMEDATA, "../evil.scd", "x", b"y")
            .is_err());
        assert!(store
            .check_needed(
                CHANNEL_GAMEDATA,
                &[FileEntry {
                    path: "../evil.scd".to_string(),
                    size: 1,
                    sha256: "x".to_string(),
                }]
            )
            .is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
