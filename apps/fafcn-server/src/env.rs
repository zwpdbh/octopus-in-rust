//! Environment-variable loading helpers.
//!
//! Loads a `.env` file (if present) at startup and provides typed helpers for
//! reading environment variables consistently across the server.

use std::{path::PathBuf, str::FromStr};

/// Load variables from a `.env` file, ignoring errors if the file is missing.
pub fn load() {
    match dotenvy::dotenv() {
        Ok(path) => tracing::debug!(path = %path.display(), ".env file loaded"),
        Err(e) => tracing::debug!("no .env file loaded: {e}"),
    }
}

/// Read an optional environment variable.
pub fn var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Read an environment variable with a fallback default.
pub fn var_or(key: &str, default: &str) -> String {
    var(key).unwrap_or_else(|| default.to_string())
}

/// Read a required environment variable, returning a clear error if missing.
pub fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("missing required environment variable: {key}"))
}

/// Parse an environment variable into any `FromStr` type.
#[allow(dead_code)]
pub fn parse<T>(key: &str) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let raw = required(key)?;
    raw.parse::<T>()
        .map_err(|e| anyhow::anyhow!("failed to parse environment variable {key}: {e}"))
}

/// Read a path environment variable, falling back to a default path.
pub fn path_or(key: &str, default: impl Into<PathBuf>) -> PathBuf {
    var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into())
}
