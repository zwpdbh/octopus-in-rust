//! Gamedata upload core, shared by the CLI and the GUI.
//!
//! Flow: hash the local directory → ask the server which files it still needs
//! → upload only those → commit the manifest. Re-running after an interrupted
//! upload therefore resumes cheaply.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use fafcn_gamedata::{
    sha256_file, validate_relative_path, FileEntry, Manifest, UploadCheckRequest,
    UploadCheckResponse, UploadCommitRequest,
};
use walkdir::WalkDir;

use crate::{api, config::ClientConfig, sync::relative_slash_path, UploadArgs};

/// Progress events emitted while uploading.
pub enum UploadProgress {
    /// The local directory was hashed.
    Scanned {
        /// Number of files found.
        files: usize,
        /// Total size in bytes.
        total_bytes: u64,
    },
    /// The server reported how many files it still needs.
    Needed {
        /// Files to upload (0 = server already has everything).
        needed: usize,
    },
    /// A single file finished uploading.
    FileUploaded {
        /// Manifest-relative path.
        path: String,
        /// 1-based index within this run.
        index: usize,
        /// Total files in this run.
        count: usize,
    },
    /// The manifest was committed; the mirror is live.
    Committed {
        /// Published patch version.
        patch_version: String,
        /// Files in the manifest.
        files: usize,
    },
}

/// What a finished upload did.
pub struct UploadSummary {
    /// Files actually uploaded (0 = server already had everything).
    pub uploaded_files: usize,
    /// Bytes uploaded.
    pub uploaded_bytes: u64,
    /// Published patch version.
    pub patch_version: String,
}

/// Run the CLI `upload` subcommand (prints progress to stdout).
pub async fn run(args: UploadArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults();
    let server = api::resolve_server(args.server, &cfg)?;
    let uploader = args
        .uploader
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let patch_version = match args.patch_version {
        Some(v) => v,
        None => {
            let detected = crate::version::detect_patch_version(&args.dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "could not detect the patch version from {}/lua.nx2; pass --patch-version",
                    args.dir.display()
                )
            })?;
            println!("Auto-detected patch version: {detected}");
            detected
        }
    };
    println!("Mirror:  {server}");
    println!("Source:  {}", args.dir.display());

    let summary = upload_gamedata(
        &server,
        &args.token,
        &args.dir,
        &patch_version,
        &uploader,
        &mut |event| match event {
            UploadProgress::Scanned { files, total_bytes } => {
                println!(
                    "Found {files} file(s), {:.1} MB total.",
                    total_bytes as f64 / 1e6
                );
            }
            UploadProgress::Needed { needed } => {
                if needed == 0 {
                    println!("Server already has every file — nothing to upload.");
                } else {
                    println!("Server needs {needed} file(s):");
                }
            }
            UploadProgress::FileUploaded { path, index, count } => {
                println!("[{index}/{count}] {path}");
            }
            UploadProgress::Committed {
                patch_version,
                files,
            } => {
                println!("Manifest committed: patch {patch_version}, {files} files.");
            }
        },
    )
    .await?;
    println!(
        "Published patch {} — the mirror is now live for everyone.",
        summary.patch_version
    );

    cfg.server = Some(server);
    cfg.save()?;
    Ok(())
}

/// Upload the complete gamedata set under `dir` to the mirror, reporting
/// progress. Only files the server lacks (or has with a different hash) are
/// transferred.
pub async fn upload_gamedata(
    server: &str,
    token: &str,
    dir: &Path,
    patch_version: &str,
    uploader: &str,
    progress: &mut dyn FnMut(UploadProgress),
) -> Result<UploadSummary> {
    if !dir.is_dir() {
        anyhow::bail!("{} is not a directory", dir.display());
    }
    if patch_version.trim().is_empty() {
        anyhow::bail!("patch version must not be empty");
    }

    let entries = scan_directory(dir)?;
    if entries.is_empty() {
        anyhow::bail!("{} contains no files", dir.display());
    }
    let total_bytes: u64 = entries.iter().map(|e| e.size).sum();
    progress(UploadProgress::Scanned {
        files: entries.len(),
        total_bytes,
    });

    let http = reqwest::Client::new();
    let needed = check_needed(&http, server, token, &entries).await?;
    progress(UploadProgress::Needed {
        needed: needed.len(),
    });

    let mut uploaded_bytes = 0_u64;
    let count = needed.len();
    for (i, entry) in needed.iter().enumerate() {
        upload_one(&http, server, token, dir, entry).await?;
        uploaded_bytes += entry.size;
        progress(UploadProgress::FileUploaded {
            path: entry.path.clone(),
            index: i + 1,
            count,
        });
    }

    let manifest = commit(
        &http,
        server,
        token,
        UploadCommitRequest {
            patch_version: patch_version.to_string(),
            uploader: uploader.to_string(),
            files: entries,
        },
    )
    .await?;
    progress(UploadProgress::Committed {
        patch_version: manifest.patch_version.clone(),
        files: manifest.files.len(),
    });

    Ok(UploadSummary {
        uploaded_files: count,
        uploaded_bytes,
        patch_version: manifest.patch_version,
    })
}

/// Hash every regular file below `dir` into a manifest entry list.
fn scan_directory(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for item in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if !item.file_type().is_file() {
            continue;
        }
        let rel = relative_slash_path(dir, item.path());
        validate_relative_path(&rel)
            .with_context(|| format!("local file has an unsupported path: {rel}"))?;
        let size = fs::metadata(item.path())?.len();
        let sha256 = sha256_file(item.path())
            .with_context(|| format!("failed to hash {}", item.path().display()))?;
        entries.push(FileEntry {
            path: rel,
            size,
            sha256,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Ask the server which of our files it still needs.
async fn check_needed(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    entries: &[FileEntry],
) -> Result<Vec<FileEntry>> {
    let url = api::api_url(server, "upload/check");
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

/// Upload one file's raw bytes with its path/hash headers.
async fn upload_one(
    http: &reqwest::Client,
    server: &str,
    token: &str,
    dir: &Path,
    entry: &FileEntry,
) -> Result<()> {
    let bytes = fs::read(dir.join(&entry.path))?;
    let url = api::api_url(server, "upload/file");
    let resp = http
        .post(&url)
        .bearer_auth(token)
        .header("x-gamedata-path", &entry.path)
        .header("x-gamedata-sha256", &entry.sha256)
        .body(bytes)
        .send()
        .await?;
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
    req: UploadCommitRequest,
) -> Result<Manifest> {
    let url = api::api_url(server, "upload/commit");
    let resp = http.post(&url).bearer_auth(token).json(&req).send().await?;
    let resp = api::ensure_success(resp).await.context("commit failed")?;
    Ok(resp.json::<Manifest>().await?)
}
