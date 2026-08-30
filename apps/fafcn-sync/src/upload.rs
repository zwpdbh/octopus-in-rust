//! Gamedata upload core, shared by the CLI and the GUI.
//!
//! Publishes every channel from the local `FAForever` root:
//!
//! - `gamedata`: only [`GAMEDATA_SYNC_FILES`] (the big patch archives),
//!   versioned by the FAF patch version from `lua.nx2`.
//! - `map-generator`: the newest [`MAP_GENERATOR_KEEP`] `MapGenerator_*.jar`
//!   files, versioned by the newest jar.
//! - `coop`: co-op mission support files (`bin/init_coop.lua`,
//!   `gamedata/lobby_coop.cop`, `gamedata/*_VO.nx2`, coop-specific archives),
//!   versioned by the fa-coop `mod_info.lua` version fetched from GitHub.
//!   Skipped when `bin/init_coop.lua` is absent or GitHub is unreachable.
//!
//! Flow per channel: hash local files → ask the server which it still needs
//! → upload only those → commit the manifest. Re-running after an
//! interrupted upload therefore resumes cheaply.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use fafcn_gamedata::{
    compare_version_strings, map_generator_jar_version, parse_mod_info_version, sha256_file,
    validate_relative_path, FileEntry, Manifest, UploadCheckRequest, UploadCheckResponse,
    UploadCommitRequest, CHANNEL_COOP, CHANNEL_GAMEDATA, CHANNEL_MAPS, CHANNEL_MAP_GENERATOR,
    FAF_STANDARD_NX2, GAMEDATA_SYNC_FILES, MAP_GENERATOR_KEEP,
};
use futures_util::StreamExt;
use walkdir::WalkDir;

use crate::{
    api,
    config::ClientConfig,
    progress::{format_bytes, format_speed, ProgressReporter, TransferUpdate},
    version, UploadArgs, UploadClientArgs, UploadMapsArgs,
};

/// GitHub raw URL of the fa-coop `mod_info.lua` (carries the coop mod
/// version, e.g. `version = 66`). Anonymous; fetched at upload time — the
/// uploader is by definition the player with upstream access.
const COOP_MOD_INFO_URL: &str =
    "https://raw.githubusercontent.com/FAForever/fa-coop/master/mod_info.lua";

/// Fetch the current coop mod version from the fa-coop repo. Best-effort:
/// `None` when GitHub is unreachable (the coop upload is then skipped).
async fn fetch_coop_version(http: &reqwest::Client) -> Option<String> {
    let body = http
        .get(COOP_MOD_INFO_URL)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    parse_mod_info_version(&body)
}

/// Progress events emitted while uploading.
pub enum UploadProgress {
    /// Started working on a channel.
    ChannelStarted {
        /// Channel id.
        channel: String,
    },
    /// A channel was skipped (e.g. no map generator installed locally).
    ChannelSkipped {
        /// Channel id.
        channel: String,
        /// Why it was skipped.
        reason: String,
    },
    /// The channel's local files were hashed.
    Scanned {
        /// Number of files to publish.
        files: usize,
        /// Total size in bytes.
        total_bytes: u64,
    },
    /// The server reported how many files it still needs.
    Needed {
        /// Files to upload (0 = server already has everything).
        needed: usize,
        /// Total bytes to upload.
        total_bytes: u64,
    },
    /// Byte-level progress within the current upload plan.
    Bytes(TransferUpdate),
    /// A single file finished uploading.
    FileUploaded {
        /// Manifest-relative path.
        path: String,
        /// 1-based index within this run.
        index: usize,
        /// Total files in this run.
        count: usize,
    },
    /// The manifest was committed; the channel is live.
    Committed {
        /// Channel id.
        channel: String,
        /// Published version.
        patch_version: String,
        /// Files in the manifest.
        files: usize,
    },
}

/// One channel that was published by an upload.
pub struct PublishedChannel {
    /// Channel id (e.g. "gamedata", "faf-client").
    pub channel: String,
    /// Published version.
    pub version: String,
}

/// What a finished upload did.
pub struct UploadSummary {
    /// Files actually uploaded across all channels.
    pub uploaded_files: usize,
    /// Bytes uploaded.
    pub uploaded_bytes: u64,
    /// Published channels.
    pub published: Vec<PublishedChannel>,
}

/// Run the CLI `upload-client` subcommand: publish one FAF client installer.
pub async fn run_client(args: UploadClientArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults(crate::BUILD_TAG);
    let server = api::resolve_server(args.server, &cfg)?;
    let uploader = args
        .uploader
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let version = match args.version {
        Some(v) => v,
        None => {
            let name = args
                .file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let detected =
                fafcn_gamedata::detect_version_from_filename(&name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "could not detect the client version from {name}; pass --version"
                    )
                })?;
            println!("Auto-detected client version: {detected}");
            detected
        }
    };
    println!("Mirror: {server}");

    let summary = upload_faf_client(
        &server,
        &args.token,
        &args.file,
        &version,
        &uploader,
        &mut print_progress,
    )
    .await?;
    for published in &summary.published {
        println!("Published {} {}", published.channel, published.version);
    }

    cfg.server = Some(server);
    cfg.save()?;
    Ok(())
}

/// Run the CLI `upload-maps` subcommand: publish a folder of FAF maps.
pub async fn run_maps(args: UploadMapsArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults(crate::BUILD_TAG);
    let server = api::resolve_server(args.server, &cfg)?;
    let uploader = args
        .uploader
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("Mirror: {server}");
    println!("Maps:   {}", args.dir.display());

    let summary = upload_maps(
        &server,
        &args.token,
        &args.dir,
        &uploader,
        &mut print_progress,
    )
    .await?;
    for published in &summary.published {
        println!("Published {} {}", published.channel, published.version);
    }

    cfg.server = Some(server);
    cfg.save()?;
    Ok(())
}

/// Upload every file below `folder` (recursively — each `name.vNNNN` map
/// folder and all its files) to the `maps` channel. The server MERGES these
/// maps into the existing manifest, replacing older versions of the same
/// maps and keeping maps uploaded by others.
pub async fn upload_maps(
    server: &str,
    token: &str,
    folder: &Path,
    uploader: &str,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<UploadSummary> {
    if !folder.is_dir() {
        anyhow::bail!("{} is not a directory", folder.display());
    }
    if uploader.trim().is_empty() {
        anyhow::bail!("uploader name must not be empty");
    }
    let mut entries = Vec::new();
    for item in WalkDir::new(folder).into_iter().filter_map(|e| e.ok()) {
        if item.file_type().is_file() {
            entries.push(hash_file(folder, item.path())?);
        }
    }
    if entries.is_empty() {
        anyhow::bail!("{} contains no files", folder.display());
    }
    let plan = ChannelPlan {
        channel: CHANNEL_MAPS,
        source_dir: folder.to_path_buf(),
        version: fafcn_gamedata::today_stamp(),
        entries,
    };

    let http = reqwest::Client::new();
    let (files, bytes) = upload_channel(&http, server, token, &plan, uploader, progress).await?;
    Ok(UploadSummary {
        uploaded_files: files,
        uploaded_bytes: bytes,
        published: vec![PublishedChannel {
            channel: CHANNEL_MAPS.to_string(),
            version: plan.version.clone(),
        }],
    })
}

/// Progress printer shared by the CLI upload subcommands.
fn print_progress(event: UploadProgress) {
    match event {
        UploadProgress::ChannelStarted { channel } => println!("== {channel} =="),
        UploadProgress::ChannelSkipped { channel, reason } => {
            println!("skipping {channel}: {reason}")
        }
        UploadProgress::Scanned { files, total_bytes } => {
            println!("found {files} file(s), {:.1} MB", total_bytes as f64 / 1e6)
        }
        UploadProgress::Needed {
            needed,
            total_bytes,
        } => {
            if needed == 0 {
                println!("server already has every file");
            } else {
                println!(
                    "server needs {needed} file(s), {:.1} MB:",
                    total_bytes as f64 / 1e6
                );
            }
        }
        UploadProgress::Bytes(update) => {
            use std::io::Write;
            print!(
                "\r{:>5.1}%  {} / {}  {}    ",
                update.percent(),
                format_bytes(update.done_bytes),
                format_bytes(update.total_bytes),
                format_speed(update.bytes_per_sec),
            );
            let _ = std::io::stdout().flush();
        }
        UploadProgress::FileUploaded { path, index, count } => {
            // Pad to overwrite the live progress line above.
            println!("\r[{index}/{count}] {path:<60}")
        }
        UploadProgress::Committed {
            patch_version,
            files,
            ..
        } => {
            println!("committed: {patch_version} ({files} files)")
        }
    }
}

/// Upload one FAF client installer file to the `faf-client` channel.
pub async fn upload_faf_client(
    server: &str,
    token: &str,
    file: &Path,
    version: &str,
    uploader: &str,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<UploadSummary> {
    if !file.is_file() {
        anyhow::bail!("{} is not a file", file.display());
    }
    if version.trim().is_empty() {
        anyhow::bail!("client version must not be empty");
    }
    if uploader.trim().is_empty() {
        anyhow::bail!("uploader name must not be empty");
    }
    let source_dir = file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", file.display()))?
        .to_path_buf();
    let plan = ChannelPlan {
        channel: fafcn_gamedata::CHANNEL_FAF_CLIENT,
        source_dir,
        version: version.to_string(),
        entries: vec![hash_file(file.parent().unwrap(), file)?],
    };

    let http = reqwest::Client::new();
    let (files, bytes) = upload_channel(&http, server, token, &plan, uploader, progress).await?;
    Ok(UploadSummary {
        uploaded_files: files,
        uploaded_bytes: bytes,
        published: vec![PublishedChannel {
            channel: fafcn_gamedata::CHANNEL_FAF_CLIENT.to_string(),
            version: version.to_string(),
        }],
    })
}

/// One channel's upload plan: what to publish under which version.
struct ChannelPlan {
    channel: &'static str,
    source_dir: std::path::PathBuf,
    version: String,
    entries: Vec<FileEntry>,
}

/// Run the CLI `upload` subcommand (prints progress to stdout).
pub async fn run(args: UploadArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults(crate::BUILD_TAG);
    let server = api::resolve_server(args.server, &cfg)?;
    let uploader = args
        .uploader
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!("Mirror:    {server}");
    println!("FAForever: {}", args.dir.display());

    let summary = upload_gamedata(
        &server,
        &args.token,
        &args.dir,
        args.patch_version.as_deref(),
        &uploader,
        &mut print_progress,
    )
    .await?;
    for published in &summary.published {
        println!("Published {} {}", published.channel, published.version);
    }

    cfg.server = Some(server);
    cfg.gamedata_dir = Some(args.dir);
    cfg.save()?;
    Ok(())
}

/// Upload every channel from the local `FAForever` root, reporting progress.
///
/// `gamedata_version_override` forces the gamedata channel version (CLI flag);
/// normally it is auto-detected from `lua.nx2`.
pub async fn upload_gamedata(
    server: &str,
    token: &str,
    faf_root: &Path,
    gamedata_version_override: Option<&str>,
    uploader: &str,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<UploadSummary> {
    if !faf_root.is_dir() {
        anyhow::bail!("{} is not a directory", faf_root.display());
    }
    if uploader.trim().is_empty() {
        anyhow::bail!("uploader name must not be empty");
    }

    let http = reqwest::Client::new();
    // Best-effort: the coop channel needs the fa-coop mod version; when
    // GitHub is unreachable the coop upload is skipped, not fatal.
    let coop_version = fetch_coop_version(&http).await;
    let mut summary = UploadSummary {
        uploaded_files: 0,
        uploaded_bytes: 0,
        published: Vec::new(),
    };

    for plan in plan_channels(
        faf_root,
        gamedata_version_override,
        coop_version.as_deref(),
        progress,
    )? {
        let (files, bytes) =
            upload_channel(&http, server, token, &plan, uploader, progress).await?;
        summary.uploaded_files += files;
        summary.uploaded_bytes += bytes;
        summary.published.push(PublishedChannel {
            channel: plan.channel.to_string(),
            version: plan.version.clone(),
        });
    }
    Ok(summary)
}

/// Build the upload plan for every channel available locally.
fn plan_channels(
    faf_root: &Path,
    gamedata_version_override: Option<&str>,
    coop_version: Option<&str>,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<Vec<ChannelPlan>> {
    let mut plans = Vec::new();

    // gamedata: the big patch archives, version from lua.nx2 (or override).
    let gamedata_dir = faf_root.join("gamedata");
    let version = match gamedata_version_override {
        Some(v) => v.to_string(),
        None => version::detect_patch_version(&gamedata_dir).ok_or_else(|| {
            anyhow::anyhow!(
                "could not detect the patch version from {}/lua.nx2",
                gamedata_dir.display()
            )
        })?,
    };
    let mut entries = Vec::new();
    for name in GAMEDATA_SYNC_FILES {
        let path = gamedata_dir.join(name);
        if !path.is_file() {
            anyhow::bail!(
                "required gamedata file {} not found in {}",
                name,
                gamedata_dir.display()
            );
        }
        entries.push(hash_file(&gamedata_dir, &path)?);
    }
    plans.push(ChannelPlan {
        channel: CHANNEL_GAMEDATA,
        source_dir: gamedata_dir,
        version,
        entries,
    });

    // map-generator: newest few jars (optional channel).
    let generator_dir = faf_root.join("map_generator");
    match newest_generator_jars(&generator_dir) {
        Some((version, entries)) => plans.push(ChannelPlan {
            channel: CHANNEL_MAP_GENERATOR,
            source_dir: generator_dir,
            version,
            entries,
        }),
        None => progress(UploadProgress::ChannelSkipped {
            channel: CHANNEL_MAP_GENERATOR.to_string(),
            reason: "no MapGenerator_*.jar found".to_string(),
        }),
    }

    // coop: mission support files (optional channel).
    match coop_version {
        Some(version) => match plan_coop(faf_root, version)? {
            Some(plan) => plans.push(plan),
            None => progress(UploadProgress::ChannelSkipped {
                channel: CHANNEL_COOP.to_string(),
                reason: "no bin/init_coop.lua found (host a coop game once first)".to_string(),
            }),
        },
        None => progress(UploadProgress::ChannelSkipped {
            channel: CHANNEL_COOP.to_string(),
            reason: "could not read the coop version from GitHub".to_string(),
        }),
    }

    Ok(plans)
}

/// Build the coop channel plan: mission support files from a FAForever
/// install where coop works, as root-relative manifest paths.
///
/// Whitelist (never touches the base-game archives owned by the gamedata
/// channel):
/// - `bin/init_coop.lua` — the coop init file (required; its absence means
///   coop was never downloaded locally and the channel is skipped).
/// - `gamedata/lobby_coop.cop` — legacy coop lobby archive (optional).
/// - `gamedata/*_VO.nx2` — voice-over banks.
/// - `gamedata/*.nx2` NOT in [`FAF_STANDARD_NX2`] — coop-specific archives
///   (e.g. `mods.nx2` packed from the fa-coop `mods/` directory). Note this
///   also sweeps in other mods' archives if the uploader has them (e.g.
///   nomads.nx2) — harmless for a friend-group mirror.
fn plan_coop(faf_root: &Path, version: &str) -> Result<Option<ChannelPlan>> {
    let init_file = faf_root.join("bin").join("init_coop.lua");
    if !init_file.is_file() {
        return Ok(None);
    }
    let mut entries = vec![hash_file(faf_root, &init_file)?];

    let gamedata_dir = faf_root.join("gamedata");
    if gamedata_dir.is_dir() {
        let legacy_cop = gamedata_dir.join("lobby_coop.cop");
        if legacy_cop.is_file() {
            entries.push(hash_file(faf_root, &legacy_cop)?);
        }
        for item in fs::read_dir(&gamedata_dir)? {
            let item = item?;
            let path = item.path();
            if !path.is_file() {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            let is_voice_over = name.ends_with("_VO.nx2");
            let is_coop_archive =
                name.ends_with(".nx2") && !FAF_STANDARD_NX2.contains(&name.as_str());
            if is_voice_over || is_coop_archive {
                entries.push(hash_file(faf_root, &path)?);
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Some(ChannelPlan {
        channel: CHANNEL_COOP,
        source_dir: faf_root.to_path_buf(),
        version: version.to_string(),
        entries,
    }))
}

/// The newest [`MAP_GENERATOR_KEEP`] generator jars and their version.
fn newest_generator_jars(dir: &Path) -> Option<(String, Vec<FileEntry>)> {
    let mut jars: Vec<(String, String)> = Vec::new(); // (file_name, version)
    for item in fs::read_dir(dir).ok()? {
        let name = item.ok()?.file_name().to_string_lossy().into_owned();
        if let Some(v) = map_generator_jar_version(&name) {
            jars.push((name, v));
        }
    }
    if jars.is_empty() {
        return None;
    }
    jars.sort_by(|a, b| compare_version_strings(&b.1, &a.1).unwrap_or(std::cmp::Ordering::Equal));
    jars.truncate(MAP_GENERATOR_KEEP);
    let version = jars.first()?.1.clone();
    let mut entries = Vec::new();
    for (name, _) in jars {
        entries.push(hash_file(dir, &dir.join(name)).ok()?);
    }
    Some((version, entries))
}

/// Hash one file into a manifest entry.
fn hash_file(dir: &Path, path: &Path) -> Result<FileEntry> {
    let rel = crate::sync::relative_slash_path(dir, path);
    validate_relative_path(&rel).with_context(|| format!("unsupported path: {rel}"))?;
    let size = fs::metadata(path)?.len();
    let sha256 = sha256_file(path).with_context(|| format!("failed to hash {}", path.display()))?;
    Ok(FileEntry {
        path: rel,
        size,
        sha256,
    })
}

/// Upload one channel per its plan. Returns (files uploaded, bytes uploaded).
async fn upload_channel(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    plan: &ChannelPlan,
    uploader: &str,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<(usize, u64)> {
    progress(UploadProgress::ChannelStarted {
        channel: plan.channel.to_string(),
    });
    let total_bytes: u64 = plan.entries.iter().map(|e| e.size).sum();
    progress(UploadProgress::Scanned {
        files: plan.entries.len(),
        total_bytes,
    });

    let needed = check_needed(http, server, token, plan.channel, &plan.entries).await?;
    let needed_bytes: u64 = needed.iter().map(|e| e.size).sum();
    progress(UploadProgress::Needed {
        needed: needed.len(),
        total_bytes: needed_bytes,
    });

    let mut uploaded_bytes = 0_u64;
    let count = needed.len();
    let mut reporter = ProgressReporter::new(needed_bytes, progress, UploadProgress::Bytes);
    for (i, entry) in needed.iter().enumerate() {
        upload_one(
            http,
            server,
            token,
            plan.channel,
            &plan.source_dir,
            entry,
            &mut reporter,
        )
        .await?;
        uploaded_bytes += entry.size;
        reporter.snapshot();
        reporter.emit(UploadProgress::FileUploaded {
            path: entry.path.clone(),
            index: i + 1,
            count,
        });
    }

    let manifest = commit(
        http,
        server,
        token,
        plan.channel,
        UploadCommitRequest {
            patch_version: plan.version.clone(),
            uploader: uploader.to_string(),
            files: plan.entries.clone(),
        },
    )
    .await?;
    progress(UploadProgress::Committed {
        channel: plan.channel.to_string(),
        patch_version: manifest.patch_version.clone(),
        files: manifest.files.len(),
    });
    Ok((count, uploaded_bytes))
}

/// Ask the server which of our files it still needs.
async fn check_needed(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    channel: &str,
    entries: &[FileEntry],
) -> Result<Vec<FileEntry>> {
    let url = api::api_url(server, &format!("channels/{channel}/upload/check"));
    let resp = http
        .post(&url)
        .bearer_auth(token)
        .json(&UploadCheckRequest {
            files: entries.to_vec(),
        })
        .send()
        .await?;
    let resp = api::ensure_success(resp)
        .await
        .context("upload check failed (is your token correct?)")?;
    let check: UploadCheckResponse = resp.json().await?;
    Ok(entries
        .iter()
        .filter(|e| check.needed.contains(&e.path))
        .cloned()
        .collect())
}

/// Request-body chunk size for streamed uploads.
const UPLOAD_CHUNK_SIZE: usize = 256 * 1024;

/// Upload one file's raw bytes with its path/hash headers. The body is
/// streamed in chunks so byte-level progress can be reported: the stream
/// counts each chunk's size into a channel, and the select loop below feeds
/// those counts into `reporter` while the request is in flight.
async fn upload_one(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    channel: &str,
    dir: &Path,
    entry: &FileEntry,
    reporter: &mut ProgressReporter<'_, UploadProgress>,
) -> Result<()> {
    // reqwest::Body::wrap_stream requires a 'static stream, so the counting
    // closure cannot borrow the reporter — it reports chunk sizes back instead.
    let bytes = bytes::Bytes::from(fs::read(dir.join(&entry.path))?);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let chunk_count = bytes.len().div_ceil(UPLOAD_CHUNK_SIZE);
    let stream = futures_util::stream::iter(0..chunk_count).map(move |i| {
        let start = i * UPLOAD_CHUNK_SIZE;
        let end = (start + UPLOAD_CHUNK_SIZE).min(bytes.len());
        let chunk = bytes.slice(start..end);
        let _ = tx.send(chunk.len() as u64);
        Ok::<_, std::io::Error>(chunk)
    });

    let url = api::api_url(server, &format!("channels/{channel}/upload/file"));
    let send = http
        .post(&url)
        .bearer_auth(token)
        // The path travels in a header, which is ASCII-only — percent-encode
        // it (non-ASCII map file names exist in the wild).
        .header("x-gamedata-path", api::encode_relative_path(&entry.path))
        .header("x-gamedata-sha256", &entry.sha256)
        .body(reqwest::Body::wrap_stream(stream))
        .send();
    tokio::pin!(send);
    let resp = loop {
        tokio::select! {
            resp = &mut send => break resp?,
            Some(n) = rx.recv() => reporter.add(n),
        }
    };
    // Drain chunks counted just before the request completed.
    while let Ok(n) = rx.try_recv() {
        reporter.add(n);
    }
    api::ensure_success(resp)
        .await
        .with_context(|| format!("failed to upload {}", entry.path))?;
    Ok(())
}

/// Commit the manifest once all needed files are stored.
async fn commit(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    channel: &str,
    req: UploadCommitRequest,
) -> Result<Manifest> {
    let url = api::api_url(server, &format!("channels/{channel}/upload/commit"));
    let resp = http.post(&url).bearer_auth(token).json(&req).send().await?;
    let resp = api::ensure_success(resp).await.context("commit failed")?;
    Ok(resp.json::<Manifest>().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_faf_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fafcn-upload-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("gamedata")).unwrap();
        root
    }

    #[test]
    fn plan_coop_collects_only_coop_files() {
        let root = temp_faf_root();
        fs::write(root.join("bin/init_coop.lua"), b"init").unwrap();
        fs::write(root.join("gamedata/lobby_coop.cop"), b"cop").unwrap();
        fs::write(root.join("gamedata/A01_VO.nx2"), b"vo").unwrap();
        fs::write(root.join("gamedata/mods.nx2"), b"coop-mods").unwrap();
        // Decoys: base-game archives owned by the gamedata channel.
        fs::write(root.join("gamedata/units.nx2"), b"base-units").unwrap();
        fs::write(root.join("gamedata/env.nx2"), b"base-env").unwrap();

        let plan = plan_coop(&root, "66").unwrap().unwrap();
        assert_eq!(plan.channel, CHANNEL_COOP);
        assert_eq!(plan.version, "66");
        let paths: Vec<&str> = plan.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "bin/init_coop.lua",
                "gamedata/A01_VO.nx2",
                "gamedata/lobby_coop.cop",
                "gamedata/mods.nx2",
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn plan_coop_skips_without_init_file() {
        let root = temp_faf_root();
        fs::write(root.join("gamedata/A01_VO.nx2"), b"vo").unwrap();
        assert!(plan_coop(&root, "66").unwrap().is_none());
        fs::remove_dir_all(&root).unwrap();
    }
}
