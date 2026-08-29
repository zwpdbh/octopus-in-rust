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
    /// FAF Client install folder (contains faf-client.exe), used for maps sync.
    #[serde(default)]
    pub faf_client_dir: Option<PathBuf>,
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
    /// The embedded address is only a fallback: it seeds a freshly downloaded
    /// binary (which has no remembered address yet), but a remembered one
    /// always wins — otherwise the user could never change the mirror in the
    /// Settings tab (the edit was saved to disk, then overwritten by the
    /// embedded address on the next launch). Dev builds have no embedded
    /// config, so the remembered address is the fallback there.
    pub fn with_embedded_defaults(mut self) -> Self {
        if self.server.is_none() {
            if let Some(embedded) = read_embedded_config() {
                if embedded.server.is_some() {
                    self.server = embedded.server;
                }
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
    config_dir().join("fafcn-sync-log.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows paths with spaces and backslashes (e.g. the default FAF Client
    /// install location `C:\Program Files\FAF Client`) must survive the
    /// config.toml round-trip byte-for-byte: TOML basic strings treat `\` as
    /// an escape character, so a serialization bug here silently corrupts
    /// every remembered Windows path.
    #[test]
    fn windows_paths_with_spaces_survive_toml_round_trip() {
        let cfg = ClientConfig {
            server: Some("https://faforever.cn:60".to_string()),
            gamedata_dir: Some(PathBuf::from(r"C:\ProgramData\FAForever")),
            faf_client_dir: Some(PathBuf::from(r"C:\Program Files\FAF Client")),
            lang: Some("zh".to_string()),
            upload_token: Some("tok en".to_string()),
            uploader: Some("player one".to_string()),
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: ClientConfig = toml::from_str(&text).unwrap();
        assert_eq!(
            back.faf_client_dir.as_deref(),
            Some(Path::new(r"C:\Program Files\FAF Client"))
        );
        assert_eq!(
            back.gamedata_dir.as_deref(),
            Some(Path::new(r"C:\ProgramData\FAForever"))
        );
        assert_eq!(back.upload_token.as_deref(), Some("tok en"));
    }
}
