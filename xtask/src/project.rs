use std::path::PathBuf;

use anyhow::{bail, Context, Result};

/// Resolve the workspace root directory.
///
/// The xtask binary lives at `<root>/target/{profile}/xtask`, so we walk up
/// three levels from `std::env::current_exe()`.
pub fn root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap())
}

/// Resolve the active qqbot data directory.
///
/// 1. Read the `.qqbot` marker file if it exists.
/// 2. Otherwise fall back to `<root>/data`.
pub fn data_dir() -> Result<PathBuf> {
    let marker = root().join(".qqbot");
    if marker.exists() {
        let contents = std::fs::read_to_string(&marker)
            .with_context(|| format!("failed to read {}", marker.display()))?;
        let path = PathBuf::from(contents.trim());
        if path.is_dir() {
            return Ok(path);
        }
    }

    let fallback = root().join("data");
    if fallback.is_dir() {
        Ok(fallback)
    } else {
        bail!(
            "could not find qqbot data directory (expected {} or a .qqbot marker)",
            fallback.display()
        )
    }
}

/// Cargo profile of the currently running xtask binary.
pub fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub fn profile_str(release: bool) -> &'static str {
    if release {
        "release"
    } else {
        "debug"
    }
}
