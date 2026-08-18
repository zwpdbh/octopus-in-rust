//! Gamedata sync core, shared by the CLI and the GUI.
//!
//! [`sync_gamedata`] syncs every channel (gamedata, map-generator) below the
//! local `FAForever` root against the server manifests, downloading only what
//! is missing or changed, and reports progress through a callback so both
//! frontends can present it their own way.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use fafcn_gamedata::{
    channel_subdir, compare_version_strings, map_generator_jar_version, sha256_bytes, sha256_file,
    validate_relative_path, FileEntry, Manifest, CHANNELS, CHANNEL_GAMEDATA, CHANNEL_MAP_GENERATOR,
    MAP_GENERATOR_KEEP,
};
use walkdir::WalkDir;

use crate::{api, config::ClientConfig, SyncArgs};

/// Temp directory (inside the channel dir) for in-progress downloads.
const TMP_DIR_NAME: &str = ".fafcn-sync-tmp";

/// Progress events emitted while syncing.
pub enum SyncProgress {
    /// Started working on a channel.
    ChannelStarted {
        /// Channel id.
        channel: String,
    },
    /// The mirror has nothing published for this channel yet.
    ChannelEmpty {
        /// Channel id.
        channel: String,
    },
    /// A channel manifest was fetched successfully.
    ManifestLoaded {
        /// Channel id.
        channel: String,
        /// Channel version, as declared by the uploader.
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
    /// An outdated local file was pruned (map-generator keeps only the
    /// newest few jars).
    Pruned {
        /// File name that was removed.
        path: String,
    },
}

/// What a finished sync did.
pub struct SyncSummary {
    /// Files downloaded and installed (0 = already up to date).
    pub downloaded_files: usize,
    /// Bytes downloaded.
    pub downloaded_bytes: u64,
    /// Local gamedata files not tracked by the manifest (left untouched).
    pub extra_files: Vec<String>,
}

/// Run the CLI `sync` subcommand (prints progress to stdout).
pub async fn run(args: SyncArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults();
    let server = api::resolve_server(args.server, &cfg)?;
    let root = resolve_faf_dir(args.dir, &cfg)?;
    println!("Mirror:    {server}");
    println!("FAForever: {}", root.display());

    let summary = sync_gamedata(&server, &root, &mut |event| match event {
        SyncProgress::ChannelStarted { channel } => println!("== {channel} =="),
        SyncProgress::ChannelEmpty { channel } => {
            println!("mirror has no {channel} yet — ask an uploader to publish it")
        }
        SyncProgress::ManifestLoaded {
            patch_version,
            uploader,
            file_count,
            total_bytes,
            ..
        } => {
            println!(
                "version {patch_version} ({file_count} files, {:.1} MB), uploaded by {uploader}",
                total_bytes as f64 / 1e6,
            );
        }
        SyncProgress::PlanReady {
            downloads,
            total_bytes,
            ..
        } => {
            if downloads == 0 {
                println!("up to date");
            } else {
                println!(
                    "downloading {downloads} file(s), {:.1} MB",
                    total_bytes as f64 / 1e6
                );
            }
        }
        SyncProgress::FileInstalled {
            path, index, count, ..
        } => {
            println!("[{index}/{count}] {path}");
        }
        SyncProgress::Pruned { path, .. } => println!("pruned old file: {path}"),
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
    cfg.gamedata_dir = Some(root);
    cfg.save()?;
    println!("Sync complete. You can start the FAF client now.");
    Ok(())
}

/// Sync every channel below `faf_root` against the mirror at `server`.
pub async fn sync_gamedata(
    server: &str,
    faf_root: &Path,
    progress: &mut dyn FnMut(SyncProgress),
) -> Result<SyncSummary> {
    let http = reqwest::Client::new();
    let mut summary = SyncSummary {
        downloaded_files: 0,
        downloaded_bytes: 0,
        extra_files: Vec::new(),
    };

    for channel in CHANNELS {
        progress(SyncProgress::ChannelStarted {
            channel: channel.to_string(),
        });
        let subdir = channel_subdir(channel).expect("known channel");
        let target_dir = faf_root.join(subdir);

        let Some(manifest) = fetch_manifest(&http, server, channel).await? else {
            progress(SyncProgress::ChannelEmpty {
                channel: channel.to_string(),
            });
            continue;
        };
        progress(SyncProgress::ManifestLoaded {
            channel: channel.to_string(),
            patch_version: manifest.patch_version.clone(),
            uploader: manifest.uploader.clone(),
            file_count: manifest.files.len(),
            total_bytes: manifest.total_size(),
        });

        fs::create_dir_all(&target_dir)
            .with_context(|| format!("failed to create {}", target_dir.display()))?;
        let (downloaded, bytes) =
            sync_channel(&http, server, channel, &target_dir, &manifest, progress).await?;
        summary.downloaded_files += downloaded;
        summary.downloaded_bytes += bytes;

        match channel {
            &CHANNEL_GAMEDATA => {
                summary.extra_files = find_extra_files(&target_dir, &manifest);
            }
            &CHANNEL_MAP_GENERATOR => {
                prune_old_jars(&target_dir, &manifest, progress)?;
            }
            _ => {}
        }
    }
    Ok(summary)
}

/// Diff one channel's target dir against its manifest and download what's
/// missing or changed. Returns (files downloaded, bytes downloaded).
async fn sync_channel(
    http: &reqwest::Client,
    server: &str,
    channel: &str,
    target_dir: &Path,
    manifest: &Manifest,
    progress: &mut dyn FnMut(SyncProgress),
) -> Result<(usize, u64)> {
    let mut downloads: Vec<&FileEntry> = Vec::new();
    for entry in &manifest.files {
        validate_relative_path(&entry.path)
            .with_context(|| format!("manifest contains unsafe path: {}", entry.path))?;
        if !local_file_matches(target_dir, entry)? {
            downloads.push(entry);
        }
    }

    let total_bytes = downloads.iter().map(|e| e.size).sum();
    progress(SyncProgress::PlanReady {
        downloads: downloads.len(),
        total_bytes,
    });

    if downloads.is_empty() {
        return Ok((0, 0));
    }
    let tmp_dir = target_dir.join(TMP_DIR_NAME);
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("failed to create {}", tmp_dir.display()))?;
    let count = downloads.len();
    for (i, entry) in downloads.iter().enumerate() {
        download_one(http, server, channel, target_dir, &tmp_dir, entry).await?;
        progress(SyncProgress::FileInstalled {
            path: entry.path.clone(),
            index: i + 1,
            count,
        });
    }
    fs::remove_dir_all(&tmp_dir).ok();
    Ok((count, total_bytes))
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
    channel: &str,
    dir: &Path,
    tmp_dir: &Path,
    entry: &FileEntry,
) -> Result<()> {
    let url = api::api_url(
        server,
        &format!(
            "channels/{channel}/files/{}",
            api::encode_relative_path(&entry.path)
        ),
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

/// Keep only the newest [`MAP_GENERATOR_KEEP`] `MapGenerator_*.jar` versions
/// locally (considering both local files and the manifest), deleting older
/// ones. Only jar files matching the generator pattern are ever touched.
fn prune_old_jars(
    dir: &Path,
    manifest: &Manifest,
    progress: &mut dyn FnMut(SyncProgress),
) -> Result<()> {
    // Newest N versions across local + manifest.
    let mut versions: Vec<String> = manifest
        .files
        .iter()
        .filter_map(|f| map_generator_jar_version(&f.path))
        .collect();
    let mut local_jars: Vec<(String, String)> = Vec::new(); // (file_name, version)
    for item in fs::read_dir(dir)? {
        let name = item?.file_name().to_string_lossy().into_owned();
        if let Some(v) = map_generator_jar_version(&name) {
            local_jars.push((name, v.clone()));
            versions.push(v);
        }
    }
    versions.sort_by(|a, b| compare_version_strings(b, a).unwrap_or(std::cmp::Ordering::Equal));
    versions.dedup();
    let keep: HashSet<&str> = versions
        .iter()
        .take(MAP_GENERATOR_KEEP)
        .map(|s| s.as_str())
        .collect();

    for (name, version) in local_jars {
        if !keep.contains(version.as_str()) {
            fs::remove_file(dir.join(&name))?;
            progress(SyncProgress::Pruned { path: name });
        }
    }
    Ok(())
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

/// Fetch a channel manifest; `Ok(None)` when the channel was never published.
async fn fetch_manifest(
    http: &reqwest::Client,
    server: &str,
    channel: &str,
) -> Result<Option<Manifest>> {
    let url = api::api_url(server, &format!("channels/{channel}/manifest.json"));
    let resp = http.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = api::ensure_success(resp).await?;
    Ok(Some(resp.json::<Manifest>().await?))
}

/// True when `path` looks like the FAF data root: it is named `FAForever`
/// and has a `gamedata` subfolder containing `.nx2` patch files.
pub fn is_valid_faf_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let name_ok = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("FAForever"));
    name_ok && contains_nx2(&path.join("gamedata"))
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

/// Directories worth checking when auto-detecting the FAForever folder.
pub fn autodetect_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(program_data) = std::env::var("ProgramData") {
        candidates.push(Path::new(&program_data).join("FAForever"));
    }
    // FAF also commonly ends up directly under a drive root.
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if root.is_dir() {
            candidates.push(root.join("FAForever"));
            candidates.push(root.join("ProgramData").join("FAForever"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(Path::new(&home).join(".faforever"));
        candidates.push(Path::new(&home).join(".local/share/FAForever"));
    }
    candidates
}

/// Find the local FAForever folder automatically: the first candidate that
/// fully validates, else the first that merely exists.
pub fn autodetect_faf_dir() -> Option<PathBuf> {
    let candidates = autodetect_candidates();
    candidates
        .iter()
        .find(|c| is_valid_faf_dir(c))
        .or_else(|| candidates.iter().find(|c| c.is_dir()))
        .cloned()
}

/// Normalize a configured/entered path: if it points at the `gamedata`
/// subfolder (config from older versions), use its `FAForever` parent.
pub fn normalize_faf_dir(path: PathBuf) -> PathBuf {
    if path.file_name().is_some_and(|n| n == "gamedata") {
        if let Some(parent) = path.parent() {
            if parent
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("FAForever"))
            {
                return parent.to_path_buf();
            }
        }
    }
    path
}

/// Resolve the FAForever directory: CLI arg > remembered config > auto-detect.
fn resolve_faf_dir(arg: Option<PathBuf>, cfg: &ClientConfig) -> Result<PathBuf> {
    if let Some(dir) = arg.or_else(|| cfg.gamedata_dir.clone()) {
        return Ok(normalize_faf_dir(dir));
    }
    if let Some(detected) = autodetect_faf_dir() {
        println!("Auto-detected FAForever directory: {}", detected.display());
        return Ok(detected);
    }
    Err(anyhow!(
        "could not find your FAForever directory; pass it once with --dir <path> (e.g. --dir \"C:\\ProgramData\\FAForever\")"
    ))
}
