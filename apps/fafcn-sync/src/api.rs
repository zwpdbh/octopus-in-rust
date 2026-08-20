//! Small HTTP/URL helpers shared by the sync and upload commands.

use anyhow::{anyhow, Context, Result};

use crate::config::ClientConfig;

pub use fafcn_gamedata::encode_relative_path;

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
