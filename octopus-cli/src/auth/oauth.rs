use crate::exception::{OctopusError, Result};
use crate::share::get_share_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

pub const KIMI_CODE_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const KIMI_CODE_OAUTH_KEY: &str = "oauth/kimi-code";
pub const DEFAULT_OAUTH_HOST: &str = "https://auth.kimi.com";
pub const REFRESH_INTERVAL_SECONDS: u64 = 60;
pub const MIN_REFRESH_THRESHOLD_SECONDS: f64 = 300.0;
pub const REFRESH_THRESHOLD_RATIO: f64 = 0.5;
pub const UNAUTHORIZED_REFRESH_RETRY_COOLDOWN_SECONDS: u64 = 300;
pub const MAX_REFRESH_RETRIES: usize = 3;

// ============================================================================
// Data structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: String,
    /// Absolute Unix timestamp when the token expires.
    pub expires_at: f64,
    pub scope: String,
    pub token_type: String,
    #[serde(default)]
    pub expires_in: f64,
}

#[derive(Debug, Clone)]
pub struct DeviceAuthorization {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: Option<u64>,
    pub interval: u64,
}

// ============================================================================
// Token storage
// ============================================================================

fn credentials_dir() -> PathBuf {
    let path = get_share_dir().join("credentials");
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }
    path
}

fn credentials_path(key: &str) -> PathBuf {
    let name = key.strip_prefix("oauth/").unwrap_or(key);
    let name = name.split('/').last().unwrap_or(name);
    credentials_dir().join(format!("{}.json", name))
}

pub fn load_tokens(key: &str) -> Option<OAuthToken> {
    let path = credentials_path(key);
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_tokens(key: &str, token: &OAuthToken) -> Result<()> {
    let path = credentials_path(key);
    let parent = path
        .parent()
        .ok_or_else(|| OctopusError::Other("Invalid credentials path".to_string()))?;

    let text = serde_json::to_string_pretty(token)
        .map_err(|e| OctopusError::Other(format!("Failed to serialize token: {}", e)))?;

    // Atomic write: write to temp file, fsync, then rename
    let temp_path = parent.join(format!(".tmp-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&temp_path, text).map_err(|e| OctopusError::Io(e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_path)
            .map_err(|e| OctopusError::Io(e))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&temp_path, perms).map_err(|e| OctopusError::Io(e))?;
    }

    std::fs::rename(&temp_path, &path).map_err(|e| OctopusError::Io(e))?;

    Ok(())
}

pub fn delete_tokens(key: &str) -> Result<()> {
    let path = credentials_path(key);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| OctopusError::Io(e))?;
    }
    Ok(())
}

// ============================================================================
// Device flow
// ============================================================================

pub async fn request_device_authorization() -> Result<DeviceAuthorization> {
    let oauth_host = crate::auth::platforms::oauth_host();
    let url = format!(
        "{}/api/oauth/device_authorization",
        oauth_host.trim_end_matches('/')
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .form(&[("client_id", KIMI_CODE_CLIENT_ID)])
        .send()
        .await
        .map_err(|e| OctopusError::Other(format!("Device authorization request failed: {}", e)))?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.map_err(|e| {
        OctopusError::Other(format!(
            "Failed to parse device authorization response: {}",
            e
        ))
    })?;

    if !status.is_success() {
        return Err(OctopusError::Other(format!(
            "Device authorization failed ({}): {:?}",
            status, body
        )));
    }

    Ok(DeviceAuthorization {
        user_code: body["user_code"].as_str().unwrap_or("").to_string(),
        device_code: body["device_code"].as_str().unwrap_or("").to_string(),
        verification_uri: body["verification_uri"].as_str().unwrap_or("").to_string(),
        verification_uri_complete: body["verification_uri_complete"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        expires_in: body["expires_in"].as_u64(),
        interval: body["interval"].as_u64().unwrap_or(5),
    })
}

pub async fn poll_device_token(
    device_code: &str,
    interval: u64,
    expires_in: Option<u64>,
) -> Result<OAuthToken> {
    let oauth_host = crate::auth::platforms::oauth_host();
    let url = format!("{}/api/oauth/token", oauth_host.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let start = Instant::now();
    let max_duration = expires_in.map(|s| Duration::from_secs(s));

    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;

        if let Some(max) = max_duration {
            if start.elapsed() > max {
                return Err(OctopusError::Other(
                    "Device authorization expired. Please try again.".to_string(),
                ));
            }
        }

        let response = client
            .post(&url)
            .form(&[
                ("client_id", KIMI_CODE_CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ])
            .send()
            .await
            .map_err(|e| OctopusError::Other(format!("Token poll request failed: {}", e)))?;

        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| OctopusError::Other(format!("Failed to parse token response: {}", e)))?;

        if status.is_success() {
            return parse_token_response(&body);
        }

        // Check for "authorization_pending" error
        if let Some(error) = body["error"].as_str() {
            if error == "authorization_pending" {
                continue;
            }
            if error == "expired_token" {
                return Err(OctopusError::Other(
                    "Device code expired. Please try again.".to_string(),
                ));
            }
            if error == "access_denied" {
                return Err(OctopusError::Other("Access denied by user.".to_string()));
            }
        }

        return Err(OctopusError::Other(format!(
            "Token poll failed ({}): {:?}",
            status, body
        )));
    }
}

pub async fn login_kimi_code() -> Result<OAuthToken> {
    let device_auth = request_device_authorization().await?;

    println!("\nPlease visit: {}", device_auth.verification_uri_complete);
    println!("Or enter this code: {}\n", device_auth.user_code);

    // Try to open browser automatically
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(mut child) = std::process::Command::new("python3")
            .args(&[
                "-c",
                &format!(
                    "import webbrowser; webbrowser.open('{}')",
                    device_auth.verification_uri_complete
                ),
            ])
            .spawn()
        {
            let _ = child.wait();
        }
    }

    let token = poll_device_token(
        &device_auth.device_code,
        device_auth.interval,
        device_auth.expires_in,
    )
    .await?;

    save_tokens(KIMI_CODE_OAUTH_KEY, &token)?;
    Ok(token)
}

// ============================================================================
// Token refresh
// ============================================================================

pub async fn refresh_token(refresh_token_str: &str) -> Result<OAuthToken> {
    let oauth_host = crate::auth::platforms::oauth_host();
    let url = format!("{}/api/oauth/token", oauth_host.trim_end_matches('/'));
    let client = reqwest::Client::new();

    for attempt in 0..MAX_REFRESH_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(2u64.pow(attempt as u32));
            tokio::time::sleep(delay).await;
        }

        let response = match client
            .post(&url)
            .form(&[
                ("client_id", KIMI_CODE_CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token_str),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if attempt == MAX_REFRESH_RETRIES - 1 {
                    return Err(OctopusError::Other(format!(
                        "Token refresh failed after {} retries: {}",
                        MAX_REFRESH_RETRIES, e
                    )));
                }
                continue;
            }
        };

        let status = response.status();
        let body: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                if attempt == MAX_REFRESH_RETRIES - 1 {
                    return Err(OctopusError::Other(format!(
                        "Token refresh failed after {} retries: {}",
                        MAX_REFRESH_RETRIES, e
                    )));
                }
                continue;
            }
        };

        if status.is_success() {
            return parse_token_response(&body);
        }

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(OctopusError::Other(
                "OAuth refresh token rejected (401/403). Please log in again.".to_string(),
            ));
        }

        // Retryable status codes
        let retryable = [429u16, 500, 502, 503, 504];
        if !retryable.contains(&status.as_u16()) {
            return Err(OctopusError::Other(format!(
                "Token refresh failed ({}): {:?}",
                status, body
            )));
        }

        if attempt == MAX_REFRESH_RETRIES - 1 {
            return Err(OctopusError::Other(format!(
                "Token refresh failed after {} retries: retryable status {}",
                MAX_REFRESH_RETRIES, status
            )));
        }
    }

    unreachable!()
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_token_response(body: &serde_json::Value) -> Result<OAuthToken> {
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| OctopusError::Other("Missing access_token in response".to_string()))?
        .to_string();

    let refresh_token = body["refresh_token"].as_str().unwrap_or("").to_string();

    let expires_in = body["expires_in"]
        .as_f64()
        .or_else(|| body["expires_in"].as_u64().map(|v| v as f64))
        .unwrap_or(0.0);

    let expires_at = if expires_in > 0.0 {
        now_secs() + expires_in
    } else {
        0.0
    };

    let scope = body["scope"].as_str().unwrap_or("").to_string();

    let token_type = body["token_type"].as_str().unwrap_or("Bearer").to_string();

    Ok(OAuthToken {
        access_token,
        refresh_token,
        expires_at,
        scope,
        token_type,
        expires_in,
    })
}

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn refresh_threshold(expires_in: f64) -> f64 {
    if expires_in > 0.0 {
        MIN_REFRESH_THRESHOLD_SECONDS.max(expires_in * REFRESH_THRESHOLD_RATIO)
    } else {
        MIN_REFRESH_THRESHOLD_SECONDS
    }
}
