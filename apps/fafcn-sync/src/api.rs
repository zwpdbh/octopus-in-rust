//! Small HTTP/URL helpers shared by the sync and upload commands.

use anyhow::{anyhow, Context, Result};

use crate::config::ClientConfig;

/// Resolve the mirror URL from the command line, falling back to the
/// remembered value from the config file.
pub fn resolve_server(arg: Option<String>, cfg: &ClientConfig) -> Result<String> {
    arg.or_else(|| cfg.server.clone())
        .map(|s| s.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow!("no server configured; pass --server <url> once"))
}

/// Build an absolute API URL below the mirror base.
pub fn api_url(server: &str, suffix: &str) -> String {
    format!("{server}/api/gamedata/{suffix}")
}

/// Percent-encode a manifest relative path for use in a URL, preserving `/`
/// separators.
pub fn encode_relative_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Return the response on success, or an error carrying the status code and
/// the server's response body (which contains the failure reason).
pub async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    Err(anyhow!("server returned {status}: {}", body.trim()))
        .with_context(|| "request failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_preserves_simple_paths() {
        assert_eq!(encode_relative_path("faf.scd"), "faf.scd");
        assert_eq!(encode_relative_path("init/lua.nxt"), "init/lua.nxt");
    }

    #[test]
    fn encode_escapes_special_chars() {
        assert_eq!(encode_relative_path("my mod.scd"), "my%20mod.scd");
        assert_eq!(encode_relative_path("地图/faf.scd").contains('/'), true);
        assert!(!encode_relative_path("地图.scd").contains('图'));
    }
}
