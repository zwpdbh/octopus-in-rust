//! Server-wide configuration loaded from environment variables.

use std::path::PathBuf;

/// Return the workspace root directory (`octopus/`).
///
/// The path is derived from the compile-time crate location; on deployed
/// hosts (where the repo does not exist) canonicalization fails, in which
/// case the unresolved path is returned — every consumer is expected to be
/// overridden via `FAF_ML_*` environment variables there.
pub fn workspace_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    root.canonicalize().unwrap_or(root)
}

/// HTTP server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Port to bind the HTTP server on.
    pub port: u16,

    /// Root of the ML data store (screenshots, labels, datasets, classes).
    pub data_dir: PathBuf,

    /// Directory containing the built Dioxus web assets.
    pub assets_dir: PathBuf,
}

impl ServerConfig {
    /// Load configuration from environment variables.
    ///
    /// Variables:
    /// - `FAF_ML_PORT` — bind port (default: `3100`).
    /// - `FAF_ML_DATA_DIR` — data store root (default: `data/faf-ml`).
    /// - `FAF_ML_WEB_DIST` — built web assets directory (default:
    ///   `target/dx/faf-ml-web/release/web/public`).
    pub fn from_env() -> crate::Result<Self> {
        let root = workspace_root();
        Ok(Self {
            port: crate::env::port_or("FAF_ML_PORT", 3100)?,
            data_dir: crate::env::path_or("FAF_ML_DATA_DIR", root.join("data/faf-ml")),
            assets_dir: crate::env::path_or(
                "FAF_ML_WEB_DIST",
                root.join("target/dx/faf-ml-web/release/web/public"),
            ),
        })
    }
}
