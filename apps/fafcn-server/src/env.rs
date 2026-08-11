//! Environment-variable loading helpers.
//!
//! Loads a `.env` file (if present) at startup and provides typed helpers for
//! reading environment variables consistently across the server.

use std::{path::PathBuf, str::FromStr};

use crate::error::{Error, Result};

/// Load variables from `.env`.
///
/// First tries `apps/fafcn-server/.env` (the crate manifest directory), then
/// falls back to searching the current working directory and its parents.
pub fn load() {
    let manifest_env = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env");
    if manifest_env.is_file() {
        let _ = dotenvy::from_path(&manifest_env);
        return;
    }
    let _ = dotenvy::dotenv();
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
pub fn required(key: &str) -> Result<String> {
    std::env::var(key)
        .map_err(|_| Error::Config(format!("missing required environment variable: {key}")))
}

/// Parse an environment variable into any `FromStr` type.
#[allow(dead_code)]
pub fn parse<T>(key: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let raw = required(key)?;
    raw.parse::<T>()
        .map_err(|e| Error::Config(format!("failed to parse environment variable {key}: {e}")))
}

/// Read a path environment variable, falling back to a default path.
pub fn path_or(key: &str, default: impl Into<PathBuf>) -> PathBuf {
    var(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into())
}
