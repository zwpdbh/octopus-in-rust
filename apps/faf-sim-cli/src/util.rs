//! Small shared helpers used by multiple CLI commands.

use std::path::PathBuf;

/// Read and deserialize a JSON file, exiting the process on failure.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", path.display(), e);
        std::process::exit(1);
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("Failed to parse {}: {}", path.display(), e);
        std::process::exit(1);
    })
}
