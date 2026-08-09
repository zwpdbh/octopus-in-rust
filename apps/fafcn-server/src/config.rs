//! Server-wide configuration loaded from environment variables.

use std::path::PathBuf;

/// Return the workspace root directory (`octopus/`).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("failed to resolve workspace root")
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
}

impl ServerConfig {
    /// Load configuration from environment variables.
    ///
    /// Variables:
    /// - `FAFCN_PORT` — bind port (default: `3000`).
    /// - `FAFCN_WEB_DIST` — built web assets directory.
    /// - `FAFCN_PORTRAITS_DIR` — unit portraits directory.
    pub fn from_env() -> anyhow::Result<Self> {
        let root = workspace_root();
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
        })
    }
}
