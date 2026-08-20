//! Persistent client configuration (server URL, gamedata dir).
//!
//! Stored at `%APPDATA%/fafcn-sync/config.toml` on Windows or
//! `$XDG_CONFIG_HOME/fafcn-sync/config.toml` / `~/.config/fafcn-sync/config.toml`
//! elsewhere, so players only pass `--server` / `--dir` once.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Remembered settings from previous runs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Mirror base URL used last time.
    pub server: Option<String>,
    /// Local FAF gamedata directory used last time.
    pub gamedata_dir: Option<PathBuf>,
    /// GUI language ("zh" / "en"), remembered across launches.
    #[serde(default)]
    pub lang: Option<String>,
    /// Upload token for the uploader (remembered locally).
    #[serde(default)]
    pub upload_token: Option<String>,
    /// Uploader display name (remembered locally).
    #[serde(default)]
    pub uploader: Option<String>,
}

impl ClientConfig {
    /// Load the config file; missing or unparsable files yield defaults.
    pub fn load() -> Self {
        let path = config_path();
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist the config, creating the config directory if needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    /// Fill unset fields from config the server embedded into this executable
    /// at download time (e.g. the mirror address the binary came from).
    /// The embedded address always wins over a remembered one: a freshly
    /// downloaded binary should talk to the mirror it came from, even if an
    /// older run remembered a different (possibly stale) address. Dev builds
    /// have no embedded config, so the remembered address is the fallback.
    pub fn with_embedded_defaults(mut self) -> Self {
        if let Some(embedded) = read_embedded_config() {
            if embedded.server.is_some() {
                self.server = embedded.server;
            }
        }
        self
    }
}

/// Read the config block the server appended to this executable, if any.
fn read_embedded_config() -> Option<fafcn_gamedata::EmbeddedConfig> {
    let exe = env::current_exe().ok()?;
    let bytes = fs::read(exe).ok()?;
    fafcn_gamedata::read_config(&bytes)
}

/// Platform config directory (`%APPDATA%/fafcn-sync` or `~/.config/fafcn-sync`).
fn config_dir() -> PathBuf {
    let base = env::var("APPDATA")
        .map(PathBuf::from)
        .or_else(|_| env::var("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|_| env::var("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("fafcn-sync")
}

/// Platform config file location.
fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Where GUI crash/exit reports are appended. GUI release builds have no
/// console, so a panic would otherwise make the window vanish silently.
pub fn crash_log_path() -> PathBuf {
    config_dir().join("crash.log")
}
