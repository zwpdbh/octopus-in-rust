use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const WIRE_PROTOCOL_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFileMetadata {
    #[serde(rename = "type")]
    pub record_type: String,
    pub protocol_version: String,
}

impl WireFileMetadata {
    pub fn new() -> Self {
        Self {
            record_type: "metadata".to_string(),
            protocol_version: WIRE_PROTOCOL_VERSION.to_string(),
        }
    }
}

impl Default for WireFileMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessageRecord {
    pub timestamp: f64,
    pub message: serde_json::Value,
}

/// File-backed wire message log (`wire.jsonl`).
///
/// Each line is a JSON object. The first line is metadata; subsequent lines
/// are wire message records with a timestamp.
#[derive(Clone)]
pub struct WireFile {
    pub path: PathBuf,
    #[allow(dead_code)]
    protocol_version: String,
}

impl WireFile {
    pub fn new(path: PathBuf) -> Self {
        let protocol_version = if path.exists() {
            Self::load_protocol_version(&path).unwrap_or_else(|| WIRE_PROTOCOL_VERSION.to_string())
        } else {
            WIRE_PROTOCOL_VERSION.to_string()
        };
        Self {
            path,
            protocol_version,
        }
    }

    pub fn is_empty(&self) -> bool {
        if !self.path.exists() {
            return true;
        }
        match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len() == 0,
            Err(_) => true,
        }
    }

    /// Append a wire message to the file.
    pub async fn append_message<T: Serialize>(&self, msg: &T) -> std::io::Result<()> {
        let record = WireMessageRecord {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
            message: serde_json::to_value(msg)?,
        };
        self.append_record(&record).await
    }

    async fn append_record(&self, record: &WireMessageRecord) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let needs_header = !self.path.exists()
            || std::fs::metadata(&self.path)
                .map(|m| m.len() == 0)
                .unwrap_or(true);

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        if needs_header {
            let metadata = WireFileMetadata::new();
            let line = serde_json::to_string(&metadata)?;
            tokio::io::AsyncWriteExt::write_all(&mut file, line.as_bytes()).await?;
            tokio::io::AsyncWriteExt::write_all(&mut file, b"\n").await?;
        }

        let line = serde_json::to_string(record)?;
        tokio::io::AsyncWriteExt::write_all(&mut file, line.as_bytes()).await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, b"\n").await?;

        Ok(())
    }

    fn load_protocol_version(path: &std::path::Path) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let metadata: Result<WireFileMetadata, _> = serde_json::from_str(line);
            if let Ok(meta) = metadata {
                return Some(meta.protocol_version);
            }
            break;
        }
        None
    }
}
