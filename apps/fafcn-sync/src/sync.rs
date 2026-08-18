//! Gamedata sync core, shared by the CLI and the GUI.
//!
//! [`sync_gamedata`] diffs a local gamedata directory against the server
//! manifest and downloads only what is missing or changed, reporting progress
//! through a callback so both frontends can present it their own way.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use fafcn_gamedata::{sha256_bytes, sha256_file, validate_relative_path, FileEntry, Manifest};
use walkdir::WalkDir;

use crate::{api, config::ClientConfig, SyncArgs};

/// Temp directory (inside the gamedata dir) for in-progress downloads.
const TMP_DIR_NAME: &str = ".fafcn-sync-tmp";

/// Progress events emitted while syncing.
pub enum SyncProgress {
    /// The mirror manifest was fetched successfully.
    ManifestLoaded {
        /// FAF patch version, as declared by the uploader.
        patch_version: String,
        /// Display name of the uploader.
        uploader: String,
        /// Number of tracked files.
        file_count: usize,
        /// Total size of all tracked files.
        total_bytes: u64,
    },
    /// The local diff is complete; these downloads are about to start.
    PlanReady {
        /// Number of files to download (0 means already up to date).
        downloads: usize,
        /// Total bytes to download.
        total_bytes: u64,
    },
    /// A single file finished downloading and was installed.
    FileInstalled {
        /// Manifest-relative path.
        path: String,
        /// 1-based index within this run.
        index: usize,
        /// Total files in this run.
        count: usize,
    },
}

/// What a finished sync did.
pub struct SyncSummary {
    /// Files downloaded and installed (0 = already up to date).
    pub downloaded_files: usize,
    /// Bytes downloaded.
    pub downloaded_bytes: u64,
    /// Local files not tracked by the manifest (left untouched).
    pub extra_files: Vec<String>,
}

/// Run the CLI `sync` subcommand (prints progress to stdout).
pub async fn run(args: SyncArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults();
    let server = api::resolve_server(args.server, &cfg)?;
    let dir = resolve_gamedata_dir(args.dir, &cfg)?;
    println!("Mirror:   {server}");
    println!("Gamedata: {}", dir.display());

    let summary = sync_gamedata(&server, &dir, &mut |event| match event {
        SyncProgress::ManifestLoaded {
            patch_version,
            uploader,
            file_count,
            total_bytes,
        } => {
            println!(
                "Mirror has patch {patch_version} ({file_count} files, {:.1} MB), uploaded by {uploader}",
                total_bytes as f64 / 1e6,
            );
        }
        SyncProgress::PlanReady {
            downloads,
            total_bytes,
        } => {
            if downloads == 0 {
                println!("Everything up to date — nothing to download.");
            } else {
                println!(
                    "Downloading {downloads} file(s), {:.1} MB total:",
                    total_bytes as f64 / 1e6
                );
            }
        }
        SyncProgress::FileInstalled { path, index, count } => {
            println!("[{index}/{count}] {path}");
        }
    })
    .await?;

    for extra in &summary.extra_files {
        println!("Note: {extra} is not in the mirror manifest (left untouched)");
    }
    if summary.downloaded_files > 0 {
        println!(
            "Downloaded {} file(s), {:.1} MB.",
            summary.downloaded_files,
            summary.downloaded_bytes as f64 / 1e6
        );
    }

    cfg.server = Some(server);
    cfg.gamedata_dir = Some(dir);
    cfg.save()?;
    println!("Sync complete. You can start the FAF client now.");
    Ok(())
}

/// Sync `dir` against the mirror at `server`, reporting progress.
pub async fn sync_gamedata(
    server: &str,
    dir: &Path,
    progress: &mut dyn FnMut(SyncProgress),
) -> Result<SyncSummary> {
    let http = reqwest::Client::new();
    let manifest = fetch_manifest(&http, server).await?;
    progress(SyncProgress::ManifestLoaded {
        patch_version: manifest.patch_version.clone(),
        uploader: manifest.uploader.clone(),
        file_count: manifest.files.len(),
        total_bytes: manifest.total_size(),
    });

    // Decide per-file actions by comparing hashes.
    let mut downloads: Vec<&FileEntry> = Vec::new();
    for entry in &manifest.files {
        validate_relative_path(&entry.path)
            .with_context(|| format!("manifest contains unsafe path: {}", entry.path))?;
        if !local_file_matches(dir, entry)? {
            downloads.push(entry);
        }
    }

    let total_bytes = downloads.iter().map(|e| e.size).sum();
    progress(SyncProgress::PlanReady {
        downloads: downloads.len(),
        total_bytes,
    });

    if !downloads.is_empty() {
        let tmp_dir = dir.join(TMP_DIR_NAME);
        fs::create_dir_all(&tmp_dir)
            .with_context(|| format!("failed to create {}", tmp_dir.display()))?;
        let count = downloads.len();
        for (i, entry) in downloads.iter().enumerate() {
            download_one(&http, server, dir, &tmp_dir, entry).await?;
            progress(SyncProgress::FileInstalled {
                path: entry.path.clone(),
                index: i + 1,
                count,
            });
        }
        fs::remove_dir_all(&tmp_dir).ok();
    }

    Ok(SyncSummary {
        downloaded_files: downloads.len(),
        downloaded_bytes: total_bytes,
        extra_files: find_extra_files(dir, &manifest),
    })
}

/// True when the local file exists with matching size and hash.
fn local_file_matches(dir: &Path, entry: &FileEntry) -> Result<bool> {
    let path = dir.join(&entry.path);
    if !path.is_file() {
        return Ok(false);
    }
    if fs::metadata(&path)?.len() != entry.size {
        return Ok(false);
    }
    let local_hash =
        sha256_file(&path).with_context(|| format!("failed to hash {}", path.display()))?;
    Ok(local_hash == entry.sha256)
}

/// Download one file to the temp dir, verify its hash, then move it into
/// place (the old file is removed only after the replacement verifies).
async fn download_one(
    http: &reqwest::Client,
    server: &str,
    dir: &Path,
    tmp_dir: &Path,
    entry: &FileEntry,
) -> Result<()> {
    let url = api::api_url(
        server,
        &format!("files/{}", api::encode_relative_path(&entry.path)),
    );
    let resp = api::ensure_success(http.get(&url).send().await?)
        .await
        .with_context(|| format!("failed to download {}", entry.path))?;
    let bytes = resp.bytes().await?;

    let actual = sha256_bytes(&bytes);
    if actual != entry.sha256 {
        return Err(anyhow!(
            "downloaded {} but its hash does not match the manifest (expected {}, got {}) — refusing to install",
            entry.path,
            entry.sha256,
            actual
        ));
    }

    let tmp_path = tmp_dir.join(&entry.path);
    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp_path, &bytes)?;

    let dest = dir.join(&entry.path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    // Windows refuses to rename over an existing file; remove it only after
    // the replacement is fully downloaded and verified.
    if dest.exists() {
        fs::remove_file(&dest)?;
    }
    fs::rename(&tmp_path, &dest)?;
    Ok(())
}

/// Local files not tracked by the manifest. Never deletes anything.
fn find_extra_files(dir: &Path, manifest: &Manifest) -> Vec<String> {
    let tracked: HashSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    let mut extras = Vec::new();
    for item in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if !item.file_type().is_file() {
            continue;
        }
        let rel = relative_slash_path(dir, item.path());
        if rel.starts_with(TMP_DIR_NAME) {
            continue;
        }
        if !tracked.contains(rel.as_str()) {
            extras.push(rel);
        }
    }
    extras
}

/// Convert an absolute path below `dir` to a forward-slash relative path.
pub fn relative_slash_path(dir: &Path, path: &Path) -> String {
    path.strip_prefix(dir)
        .unwrap_or(path)
        .iter()
        .map(|c| c.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Fetch and parse the server manifest.
async fn fetch_manifest(http: &reqwest::Client, server: &str) -> Result<Manifest> {
    let url = api::api_url(server, "manifest.json");
    let resp = http.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(anyhow!(
            "the mirror has no gamedata yet — ask a player with VPN access to upload the latest patch first"
        ));
    }
    let resp = api::ensure_success(resp).await?;
    Ok(resp.json::<Manifest>().await?)
}

/// True when `path` looks like a FAF gamedata directory: it is named
/// `gamedata` below a `FAForever` directory and contains `.nx2` patch files.
pub fn is_valid_gamedata_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let name_ok = path.file_name().is_some_and(|n| n == "gamedata")
        && path
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("FAForever"));
    name_ok && contains_nx2(path)
}

/// True when the directory contains at least one `.nx2` file (top level).
fn contains_nx2(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut rd| {
            rd.any(|e| {
                e.ok()
                    .is_some_and(|e| e.path().extension().is_some_and(|ext| ext == "nx2"))
            })
        })
        .unwrap_or(false)
}

/// Directories worth checking when auto-detecting the gamedata folder.
pub fn autodetect_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(program_data) = std::env::var("ProgramData") {
        candidates.push(Path::new(&program_data).join("FAForever").join("gamedata"));
    }
    // FAF also commonly ends up directly under a drive root.
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if root.is_dir() {
            candidates.push(root.join("FAForever").join("gamedata"));
            candidates.push(root.join("ProgramData").join("FAForever").join("gamedata"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(Path::new(&home).join(".faforever").join("gamedata"));
        candidates.push(
            Path::new(&home)
                .join(".local/share/FAForever")
                .join("gamedata"),
        );
    }
    candidates
}

/// Find the local gamedata directory automatically: the first candidate that
/// fully validates, else the first that merely exists.
pub fn autodetect_gamedata_dir() -> Option<PathBuf> {
    let candidates = autodetect_candidates();
    candidates
        .iter()
        .find(|c| is_valid_gamedata_dir(c))
        .or_else(|| candidates.iter().find(|c| c.is_dir()))
        .cloned()
}

/// Resolve the gamedata directory: CLI arg > remembered config > auto-detect.
fn resolve_gamedata_dir(arg: Option<PathBuf>, cfg: &ClientConfig) -> Result<PathBuf> {
    if let Some(dir) = arg.or_else(|| cfg.gamedata_dir.clone()) {
        return Ok(dir);
    }
    if let Some(detected) = autodetect_gamedata_dir() {
        println!("Auto-detected gamedata directory: {}", detected.display());
        return Ok(detected);
    }
    Err(anyhow!(
        "could not find your FAF gamedata directory; pass it once with --dir <path> (e.g. --dir \"C:\\ProgramData\\FAForever\\gamedata\")"
    ))
}
