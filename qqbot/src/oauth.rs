use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error};

const KIMI_CODE_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const KIMI_CODE_OAUTH_HOST: &str = "https://auth.kimi.com";
const MIN_REFRESH_THRESHOLD_SECONDS: f64 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: f64,
    #[serde(default)]
    pub scope: String,
    #[serde(default = "default_bearer")]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: f64,
}

fn default_bearer() -> String {
    "Bearer".to_string()
}

pub fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

pub async fn resolve_token(token_file: &str) -> Result<String> {
    let path = expand_path(token_file);
    let mut token = load_token(&path)
        .await?
        .with_context(|| format!("OAuth token file not found: {}", path.display()))?;

    if needs_refresh(&token) {
        if token.refresh_token.is_empty() {
            anyhow::bail!("OAuth token expired and no refresh token is available");
        }
        debug!("refreshing OAuth token");
        token = refresh_token(&token.refresh_token).await?;
        save_token(&path, &token).await?;
    }

    Ok(token.access_token)
}

async fn load_token(path: &Path) -> Result<Option<OAuthToken>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read OAuth token file: {}", path.display()))?;
    let token: OAuthToken = serde_json::from_str(&text)
        .with_context(|| format!("invalid OAuth token file: {}", path.display()))?;
    Ok(Some(token))
}

async fn save_token(path: &Path, token: &OAuthToken) -> Result<()> {
    let parent = path
        .parent()
        .context("OAuth token file has no parent directory")?;
    tokio::fs::create_dir_all(parent).await?;
    let text = serde_json::to_string_pretty(token)?;
    let temp = parent.join(format!(".tmp-{}.json", uuid::Uuid::new_v4()));
    tokio::fs::write(&temp, text).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&temp).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(&temp, perms).await?;
    }
    tokio::fs::rename(&temp, path).await?;
    Ok(())
}

fn needs_refresh(token: &OAuthToken) -> bool {
    if token.expires_at <= 0.0 {
        return false;
    }
    token.expires_at - now_secs() < MIN_REFRESH_THRESHOLD_SECONDS
}

async fn refresh_token(refresh_token_str: &str) -> Result<OAuthToken> {
    let url = format!(
        "{}/api/oauth/token",
        KIMI_CODE_OAUTH_HOST.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .form(&[
            ("client_id", KIMI_CODE_CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token_str),
        ])
        .send()
        .await
        .context("OAuth refresh request failed")?;

    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to parse OAuth refresh response")?;

    if !status.is_success() {
        error!(status = %status, body = %body, "OAuth token refresh failed");
        if status.as_u16() == 401 || status.as_u16() == 403 {
            anyhow::bail!("OAuth refresh token was rejected; please log in again with octopus-cli");
        }
        anyhow::bail!("OAuth token refresh failed (HTTP {}): {}", status, body);
    }

    let mut token: OAuthToken = serde_json::from_value(body)?;
    if token.expires_in > 0.0 && token.expires_at == 0.0 {
        token.expires_at = now_secs() + token.expires_in;
    }
    Ok(token)
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
