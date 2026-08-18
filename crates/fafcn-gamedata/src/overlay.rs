//! Config embedded into downloaded client binaries.
//!
//! Both PE (Windows) and ELF executables ignore data appended after the
//! image, so the server patches the sync client *per download request* with
//! the mirror's own address. The client reads its own executable at startup
//! and uses the embedded values as defaults — the user never types a URL.
//!
//! Overlay layout: `[binary][json][len: u64 LE][magic: "FAFCNCFG"]`.

use serde::{Deserialize, Serialize};

/// Trailing magic identifying an embedded config block.
const MAGIC: &[u8; 8] = b"FAFCNCFG";

/// Sanity cap on the embedded JSON size.
const MAX_CONFIG_LEN: u64 = 64 * 1024;

/// Values the server embeds into the client binary at download time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddedConfig {
    /// Mirror base URL the binary was downloaded from.
    #[serde(default)]
    pub server: Option<String>,
}

/// Append `config` to `binary`, returning a new buffer.
pub fn append_config(binary: &[u8], config: &EmbeddedConfig) -> Result<Vec<u8>, serde_json::Error> {
    let json = serde_json::to_vec(config)?;
    let mut out = Vec::with_capacity(binary.len() + json.len() + 16);
    out.extend_from_slice(binary);
    out.extend_from_slice(&json);
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(MAGIC);
    Ok(out)
}

/// Extract the embedded config, or `None` for an unpatched binary.
pub fn read_config(binary: &[u8]) -> Option<EmbeddedConfig> {
    if binary.len() < 16 || &binary[binary.len() - 8..] != MAGIC {
        return None;
    }
    let len_bytes: [u8; 8] = binary[binary.len() - 16..binary.len() - 8]
        .try_into()
        .ok()?;
    let len = u64::from_le_bytes(len_bytes);
    if len > MAX_CONFIG_LEN || binary.len() < 16 + len as usize {
        return None;
    }
    let len = len as usize;
    let json = &binary[binary.len() - 16 - len..binary.len() - 16];
    serde_json::from_slice(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let binary = b"fake-exe-bytes";
        let config = EmbeddedConfig {
            server: Some("https://mirror.example.com".to_string()),
        };
        let patched = append_config(binary, &config).unwrap();
        // Original bytes are untouched at the front.
        assert!(patched.starts_with(binary));
        let read = read_config(&patched).unwrap();
        assert_eq!(read.server.as_deref(), Some("https://mirror.example.com"));
    }

    #[test]
    fn unpatched_binary_yields_none() {
        assert!(read_config(b"just some exe bytes, no config").is_none());
        assert!(read_config(b"").is_none());
        // Right magic, but no room for a length field.
        assert!(read_config(b"FAFCNCFG").is_none());
    }
}
