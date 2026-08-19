//! Server-wide configuration loaded from environment variables.

use std::path::PathBuf;

/// Return the workspace root directory (`octopus/`).
///
/// The path is derived from the compile-time crate location, so on deployed
/// hosts (where the repo does not exist) canonicalization fails; in that case
/// fall back to the unresolved path — every consumer is expected to be
/// overridden via `FAFCN_*` environment variables there.
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

    /// Directory containing the built Dioxus web assets.
    pub assets_dir: PathBuf,

    /// Directory containing unit portrait PNGs.
    pub portraits_dir: PathBuf,

    /// Root directory of the gamedata mirror (manifest, files, incoming).
    pub gamedata_dir: PathBuf,

    /// Directory containing downloadable sync client binaries.
    pub gamedata_client_dir: PathBuf,

    /// Bearer token required for gamedata uploads; `None` disables upload.
    pub gamedata_upload_token: Option<String>,
}

impl ServerConfig {
    /// Load configuration from environment variables.
    ///
    /// Variables:
    /// - `FAFCN_PORT` — bind port (default: `3000`).
    /// - `FAFCN_WEB_DIST` — built web assets directory.
    /// - `FAFCN_PORTRAITS_DIR` — unit portraits directory.
    /// - `FAFCN_GAMEDATA_DIR` — gamedata mirror root (default: `data/faf-gamedata`).
    /// - `FAFCN_GAMEDATA_CLIENT_DIR` — sync client binaries (default: `<gamedata>/client`).
    /// - `FAFCN_GAMEDATA_UPLOAD_TOKEN` — bearer token for uploads (optional).
    pub fn from_env() -> crate::Result<Self> {
        let root = workspace_root();
        let gamedata_dir =
            crate::env::path_or("FAFCN_GAMEDATA_DIR", root.join("data/faf-gamedata"));
        let gamedata_client_dir =
            crate::env::path_or("FAFCN_GAMEDATA_CLIENT_DIR", gamedata_dir.join("client"));
        Ok(Self {
            port: crate::env::var_or("FAFCN_PORT", "3000").parse()?,
            assets_dir: crate::env::path_or(
                "FAFCN_WEB_DIST",
                root.join("target/dx/fafcn-web/release/web/public"),
            ),
            portraits_dir: crate::env::path_or(
                "FAFCN_PORTRAITS_DIR",
                root.join("assets/icons/units"),
            ),
            gamedata_dir,
            gamedata_client_dir,
            gamedata_upload_token: crate::env::var("FAFCN_GAMEDATA_UPLOAD_TOKEN"),
        })
    }
}
