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
///
/// The body is sanitized before it goes into the error: proxies answer with
/// gzipped/binary HTML error pages, which would otherwise turn the GUI log
/// into mojibake.
pub async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    Err(anyhow!(
        "server returned {status}: {}",
        sanitize_body(&body)
    ))
    .with_context(|| "request failed".to_string())
}

/// Keep at most 200 printable chars of a server error body; a body that is
/// mostly control chars / replacement chars (e.g. a gzipped proxy error page
/// decoded lossy) collapses to `<binary response>`.
fn sanitize_body(body: &str) -> String {
    let is_bad = |c: char| (c.is_control() && !matches!(c, '\n' | '\t' | '\r')) || c == '\u{FFFD}';
    let total = body.chars().count();
    let bad = body.chars().filter(|c| is_bad(*c)).count();
    if total > 0 && bad * 10 > total {
        return "<binary response>".to_string();
    }
    body.chars()
        .filter(|c| !is_bad(*c))
        .take(200)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_body_keeps_plain_text() {
        assert_eq!(sanitize_body("channel not found"), "channel not found");
        assert_eq!(sanitize_body("  trimmed  "), "trimmed");
    }

    #[test]
    fn sanitize_body_truncates_long_text() {
        let long = "x".repeat(500);
        assert_eq!(sanitize_body(&long).len(), 200);
    }

    #[test]
    fn sanitize_body_collapses_binary() {
        // A gzipped proxy error page is mostly non-printable bytes.
        let binary: String = (0u8..=255).map(|b| b as char).collect();
        assert_eq!(sanitize_body(&binary), "<binary response>");
    }
}
