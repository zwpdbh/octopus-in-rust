//! `fafcn-sync upload`: publish a complete gamedata patch set to the mirror.
//!
//! Flow: hash the local directory → ask the server which files it still needs
//! → upload only those → commit the manifest. Re-running after an interrupted
//! upload therefore resumes cheaply.

use std::fs;

use anyhow::{Context, Result};
use fafcn_gamedata::{
    sha256_file, validate_relative_path, FileEntry, Manifest, UploadCheckRequest,
    UploadCheckResponse, UploadCommitRequest,
};
use walkdir::WalkDir;

use crate::{api, config::ClientConfig, sync::relative_slash_path, UploadArgs};

/// Run the upload command.
pub async fn run(args: UploadArgs) -> Result<()> {
    let mut cfg = ClientConfig::load().with_embedded_defaults();
    let server = api::resolve_server(args.server, &cfg)?;
    let uploader = args
        .uploader
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    if !args.dir.is_dir() {
        anyhow::bail!("{} is not a directory", args.dir.display());
    }
    println!("Mirror:  {server}");
    println!("Source:  {}", args.dir.display());

    let entries = scan_directory(&args.dir)?;
    if entries.is_empty() {
        anyhow::bail!("{} contains no files", args.dir.display());
    }
    let total: u64 = entries.iter().map(|e| e.size).sum();
    println!(
        "Found {} file(s), {:.1} MB total.",
        entries.len(),
        total as f64 / 1e6
    );

    let http = reqwest::Client::new();
    let needed = check_needed(&http, &server, &args.token, &entries).await?;
    if needed.is_empty() {
        println!("Server already has every file — nothing to upload.");
    } else {
        println!("Server needs {} file(s):", needed.len());
        for (i, entry) in needed.iter().enumerate() {
            upload_one(
                &http,
                &server,
                &args.token,
                &args.dir,
                entry,
                i + 1,
                needed.len(),
            )
            .await?;
        }
    }

    let manifest = commit(
        &http,
        &server,
        &args.token,
        UploadCommitRequest {
            patch_version: args.patch_version,
            uploader,
            files: entries,
        },
    )
    .await?;
    println!(
        "Published patch {} ({} files) — the mirror is now live for everyone.",
        manifest.patch_version,
        manifest.files.len()
    );

    cfg.server = Some(server);
    cfg.save()?;
    Ok(())
}

/// Hash every regular file below `dir` into a manifest entry list.
fn scan_directory(dir: &std::path::Path) -> Result<Vec<FileEntry>> {
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
        .context("upload check failed (is your --token correct?)")?;
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
    dir: &std::path::Path,
    entry: &FileEntry,
    index: usize,
    count: usize,
) -> Result<()> {
    println!(
        "[{index}/{count}] {} ({:.1} MB)",
        entry.path,
        entry.size as f64 / 1e6
    );
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
