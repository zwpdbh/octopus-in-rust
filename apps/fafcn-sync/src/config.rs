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
    /// Remembered settings always win over embedded ones — with one
    /// exception: a remembered plain-`http` address is upgraded when the
    /// binary was downloaded from the same host over `https` (the mirror
    /// moved behind a TLS proxy; the old `http` address no longer works).
    pub fn with_embedded_defaults(mut self) -> Self {
        if let Some(embedded) = read_embedded_config() {
            match (&self.server, &embedded.server) {
                (None, _) => self.server = embedded.server,
                (Some(remembered), Some(embedded_server))
                    if is_scheme_upgrade(remembered, embedded_server) =>
                {
                    self.server = Some(embedded_server.clone());
                }
                _ => {}
            }
        }
        self
    }
}

/// True when `remembered` and `embedded` are the same address except that
/// `remembered` uses `http://` and `embedded` uses `https://`.
fn is_scheme_upgrade(remembered: &str, embedded: &str) -> bool {
    match (
        remembered.strip_prefix("http://"),
        embedded.strip_prefix("https://"),
    ) {
        (Some(old_rest), Some(new_rest)) => old_rest == new_rest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_scheme_upgrade;

    #[test]
    fn scheme_upgrade_detected() {
        assert!(is_scheme_upgrade(
            "http://8v.pub:10041",
            "https://8v.pub:10041"
        ));
    }

    #[test]
    fn different_host_is_not_an_upgrade() {
        assert!(!is_scheme_upgrade(
            "http://8v.pub:10041",
            "https://mirror.example.com"
        ));
        assert!(!is_scheme_upgrade(
            "http://8v.pub:10041",
            "https://8v.pub:9999"
        ));
    }

    #[test]
    fn https_remembered_is_never_downgraded() {
        assert!(!is_scheme_upgrade(
            "https://8v.pub:10041",
            "http://8v.pub:10041"
        ));
        assert!(!is_scheme_upgrade(
            "https://8v.pub:10041",
            "https://8v.pub:10041"
        ));
    }
}

/// Read the config block the server appended to this executable, if any.
fn read_embedded_config() -> Option<fafcn_gamedata::EmbeddedConfig> {
    let exe = env::current_exe().ok()?;
    let bytes = fs::read(exe).ok()?;
    fafcn_gamedata::read_config(&bytes)
}

/// Platform config file location.
fn config_path() -> PathBuf {
    let base = env::var("APPDATA")
        .map(PathBuf::from)
        .or_else(|_| env::var("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|_| env::var("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("fafcn-sync").join("config.toml")
}
