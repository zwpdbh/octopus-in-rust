//! On-disk storage for the gamedata mirror: uploaded files plus the manifest.
//!
//! Layout below the configured root:
//!
//! ```text
//! <root>/
//!   manifest.json   # generated atomically on commit, never hand-edited
//!   files/<path>    # content served to sync clients
//!   incoming/       # temp dir for in-progress uploads (renamed into files/)
//! ```

use std::{fs, path::PathBuf};

use axum::http::HeaderMap;
use chrono::Utc;
use fafcn_gamedata::{
    sha256_bytes, sha256_file, validate_relative_path, FileEntry, Manifest, UploadCommitRequest,
};

use crate::error::{Error, Result};

/// Owns the gamedata mirror directory and the upload credential.
#[derive(Debug)]
pub struct GamedataStore {
    root: PathBuf,
    upload_token: Option<String>,
}

impl GamedataStore {
    /// Create the store, ensuring the directory layout exists.
    pub fn new(root: PathBuf, upload_token: Option<String>) -> Result<Self> {
        fs::create_dir_all(root.join("files"))?;
        fs::create_dir_all(root.join("incoming"))?;
        Ok(Self { root, upload_token })
    }

    /// Directory whose contents are served under `/api/gamedata/files`.
    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    fn incoming_dir(&self) -> PathBuf {
        self.root.join("incoming")
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
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

    /// Read the current manifest, or `None` if nothing was ever committed.
    pub fn read_manifest(&self) -> Result<Option<Manifest>> {
        let path = self.manifest_path();
        if !path.is_file() {
            return Ok(None);
        }
        let json = fs::read_to_string(&path)?;
        let manifest = serde_json::from_str(&json)
            .map_err(|e| Error::Internal(format!("corrupt manifest.json: {e}")))?;
        Ok(Some(manifest))
    }

    /// Return the subset of `files` the server does not already have stored
    /// with a matching hash.
    pub fn check_needed(&self, files: &[FileEntry]) -> Result<Vec<String>> {
        let mut needed = Vec::new();
        for entry in files {
            validate_relative_path(&entry.path).map_err(|e| Error::BadRequest(e.to_string()))?;
            if !self.stored_file_matches(entry)? {
                needed.push(entry.path.clone());
            }
        }
        Ok(needed)
    }

    /// Store one uploaded file: hash-verify, then atomically move from
    /// `incoming/` into `files/`.
    pub fn store_upload(&self, rel_path: &str, expected_sha256: &str, bytes: &[u8]) -> Result<()> {
        validate_relative_path(rel_path).map_err(|e| Error::BadRequest(e.to_string()))?;
        let actual = sha256_bytes(bytes);
        if actual != expected_sha256 {
            return Err(Error::BadRequest(format!(
                "sha256 mismatch for {rel_path}: expected {expected_sha256}, got {actual}"
            )));
        }

        let tmp = self
            .incoming_dir()
            .join(format!("{}.part", uuid::Uuid::new_v4()));
        fs::write(&tmp, bytes)?;

        let dest = self.files_dir().join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&tmp, &dest)?;
        Ok(())
    }

    /// Finalize an upload session: verify every listed file is present with a
    /// matching hash, then atomically replace `manifest.json`.
    pub fn commit(&self, req: &UploadCommitRequest) -> Result<Manifest> {
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
            if !self.stored_file_matches(entry)? {
                return Err(Error::BadRequest(format!(
                    "cannot commit: {} is missing or does not match its sha256",
                    entry.path
                )));
            }
        }

        let manifest = Manifest {
            patch_version: req.patch_version.clone(),
            uploader: req.uploader.clone(),
            generated_at: Utc::now(),
            files: req.files.clone(),
        };
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| Error::Internal(format!("failed to serialize manifest: {e}")))?;
        let tmp = self.root.join("manifest.json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, self.manifest_path())?;
        Ok(manifest)
    }

    /// True when `files/<path>` exists with matching size and hash.
    fn stored_file_matches(&self, entry: &FileEntry) -> Result<bool> {
        let path = self.files_dir().join(&entry.path);
        if !path.is_file() {
            return Ok(false);
        }
        if fs::metadata(&path)?.len() != entry.size {
            return Ok(false);
        }
        Ok(sha256_file(&path)? == entry.sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Nothing stored yet: the file is needed.
        assert_eq!(
            store.check_needed(&[entry.clone()]).unwrap(),
            vec!["faf.scd"]
        );

        // Upload with a wrong hash is rejected and stores nothing.
        assert!(store
            .store_upload("faf.scd", "00".repeat(32).as_str(), bytes)
            .is_err());

        // Correct upload: no longer needed, committable.
        store.store_upload("faf.scd", &entry.sha256, bytes).unwrap();
        assert!(store
            .check_needed(std::slice::from_ref(&entry))
            .unwrap()
            .is_empty());

        let manifest = store
            .commit(&UploadCommitRequest {
                patch_version: "3825".to_string(),
                uploader: "tester".to_string(),
                files: vec![entry],
            })
            .unwrap();
        assert_eq!(manifest.patch_version, "3825");

        let loaded = store.read_manifest().unwrap().unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.files[0].sha256, manifest.files[0].sha256);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commit_rejects_missing_files() {
        let (root, store) = temp_store();
        let result = store.commit(&UploadCommitRequest {
            patch_version: "3825".to_string(),
            uploader: "tester".to_string(),
            files: vec![entry_for(b"nope")],
        });
        assert!(result.is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_path_traversal() {
        let (root, store) = temp_store();
        assert!(store.store_upload("../evil.scd", "x", b"y").is_err());
        assert!(store
            .check_needed(&[FileEntry {
                path: "../evil.scd".to_string(),
                size: 1,
                sha256: "x".to_string(),
            }])
            .is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
