//! Self-update: check the mirror for a newer fafcn-sync build, download it,
//! and swap it in place.
//!
//! The mirror serves the newest exe at
//! `GET /api/gamedata/client/fafcn-sync-x86_64-pc-windows-gnu.exe` and its
//! build tag at `GET /api/gamedata/status` (`StatusResponse.client_tag`).
//! A running Windows exe cannot be overwritten, but it *can* be renamed, so
//! the swap is: rename current exe aside, move the download into its place,
//! relaunch, and delete the leftover on the next startup.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::api;

/// File name of the Windows client served by the mirror (kept in sync with
/// `SYNC_CLIENT_FILE_NAME` in `xtask/src/apps/fafcn.rs`).
const CLIENT_EXE_NAME: &str = "fafcn-sync-x86_64-pc-windows-gnu.exe";

/// Suffix of the freshly downloaded exe, next to the running one.
const NEW_SUFFIX: &str = "new";
/// Suffix of the previous exe renamed aside during the swap.
const OLD_SUFFIX: &str = "old";

/// Fetch the build tag of the client the mirror currently serves, or `None`
/// when the server has none (or an older server without the field).
pub async fn fetch_client_tag(server: &str) -> Result<Option<String>> {
    let url = api::api_url(server, "status");
    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("GET {url}"))?;
    let status: fafcn_gamedata::StatusResponse = api::ensure_success(resp)
        .await?
        .json()
        .await
        .context("invalid status response")?;
    Ok(status.client_tag)
}

/// True when `server_tag` is a newer build than `local_tag`.
///
/// Build tags look like `dev-{unix_secs:08x}-{rand:04x}` (see
/// `xtask fafcn file-sync`): newer means a strictly larger timestamp.
/// Unparseable tags fall back to a plain inequality check. A local `"dev"`
/// build (no stamp) is never considered outdated, so developers are not nagged.
pub fn is_newer_build(server_tag: &str, local_tag: &str) -> bool {
    if local_tag == "dev" {
        return false;
    }
    match (tag_timestamp(server_tag), tag_timestamp(local_tag)) {
        (Some(server), Some(local)) => server > local,
        _ => server_tag != local_tag,
    }
}

/// The hex timestamp part of a `dev-{ts:08x}-{rand:04x}` build tag.
fn tag_timestamp(tag: &str) -> Option<u32> {
    let ts = tag.strip_prefix("dev-")?.split('-').next()?;
    u32::from_str_radix(ts, 16).ok()
}

/// Path the freshly downloaded exe is written to (`<current_exe>.new`).
pub fn new_exe_path() -> Result<PathBuf> {
    suffixed_exe_path(NEW_SUFFIX)
}

/// `<current_exe>.<suffix>` — sibling of the running executable.
fn suffixed_exe_path(suffix: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot locate own executable")?;
    let mut name = exe.file_name().expect("exe has a file name").to_os_string();
    name.push(format!(".{suffix}"));
    Ok(exe.with_file_name(name))
}

/// Delete the `<current_exe>.old` leftover from a previous self-update.
/// Best-effort: called at startup, never fails.
pub fn cleanup_old_exe() {
    if let Ok(old) = suffixed_exe_path(OLD_SUFFIX) {
        let _ = std::fs::remove_file(old);
    }
}

/// Download the newest client exe from the mirror to `dest`, reporting
/// `(done_bytes, total_bytes)` through `progress` (`total` is 0 when the
/// server sends no length).
pub async fn download_client(
    server: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let url = api::api_url(server, &format!("client/{CLIENT_EXE_NAME}"));
    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("GET {url}"))?;
    let mut resp = api::ensure_success(resp).await?;
    let total = resp.content_length().unwrap_or(0);

    let mut file =
        std::fs::File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await.context("download interrupted")? {
        use std::io::Write;
        file.write_all(&chunk)?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    Ok(())
}

/// Swap `new_exe` in place of the running executable and relaunch.
///
/// On success this function never returns: the new process is spawned and
/// the current one exits. On failure the running exe is left untouched and
/// the user can fall back to downloading from the website.
pub fn apply_and_restart(new_exe: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("cannot locate own executable")?;
    let old = suffixed_exe_path(OLD_SUFFIX)?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).with_context(|| format!("cannot move aside {}", exe.display()))?;
    if let Err(e) = std::fs::rename(new_exe, &exe) {
        // Roll back so the user is not left without a working exe.
        let _ = std::fs::rename(&old, &exe);
        return Err(e).with_context(|| format!("cannot install {}", new_exe.display()));
    }
    std::process::Command::new(&exe)
        .spawn()
        .with_context(|| format!("cannot relaunch {}", exe.display()))?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_timestamp_is_an_update() {
        assert!(is_newer_build("dev-68f3a1c3-9b4e", "dev-68f3a1c2-0001"));
    }

    #[test]
    fn same_or_older_is_not_an_update() {
        assert!(!is_newer_build("dev-68f3a1c2-9b4e", "dev-68f3a1c2-0001"));
        assert!(!is_newer_build("dev-68f3a1c1-9b4e", "dev-68f3a1c2-0001"));
        assert!(!is_newer_build("dev-68f3a1c2-9b4e", "dev-68f3a1c2-9b4e"));
    }

    #[test]
    fn unversioned_dev_build_is_never_outdated() {
        assert!(!is_newer_build("dev-68f3a1c3-9b4e", "dev"));
    }

    #[test]
    fn unparseable_tags_fall_back_to_inequality() {
        assert!(is_newer_build("release-1", "release-2"));
        assert!(!is_newer_build("release-1", "release-1"));
        assert!(is_newer_build("dev-xyz-1", "dev-68f3a1c2-0001"));
    }
}
