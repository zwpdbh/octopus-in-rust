//! Manifest and upload-protocol types shared by server and client.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One file tracked by the gamedata mirror.
///
/// `path` is a forward-slash relative path below the FAF `gamedata` directory
/// (e.g. `faf.scd` or `init/lua.nxt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path below the gamedata directory.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the file contents.
    pub sha256: String,
}

/// The mirror manifest: the single source of truth clients diff against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// FAF patch version these files correspond to, as declared by the uploader.
    pub patch_version: String,
    /// Display name of the player who uploaded this set.
    pub uploader: String,
    /// When the manifest was committed on the server.
    pub generated_at: DateTime<Utc>,
    /// All tracked files.
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// Total size of all tracked files in bytes.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Build a summary for the status endpoint.
    pub fn summary(&self) -> ManifestSummary {
        ManifestSummary {
            patch_version: self.patch_version.clone(),
            uploader: self.uploader.clone(),
            file_count: self.files.len(),
            total_size: self.total_size(),
            last_updated: self.generated_at,
        }
    }
}

/// Abridged manifest information for the status endpoint and web page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSummary {
    /// FAF patch version, as declared by the uploader.
    pub patch_version: String,
    /// Display name of the player who uploaded this set.
    pub uploader: String,
    /// Number of tracked files.
    pub file_count: usize,
    /// Total size of all tracked files in bytes.
    pub total_size: u64,
    /// When the manifest was last committed.
    pub last_updated: DateTime<Utc>,
}

/// Response of `GET /api/gamedata/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Current mirror contents; `None` when nothing has been uploaded yet.
    pub manifest: Option<ManifestSummary>,
    /// Build tag of the sync client binary currently served, so the web page
    /// can show users which build they will download.
    #[serde(default)]
    pub client_tag: Option<String>,
}

/// Request of `POST /api/gamedata/upload/check`: which of these files the
/// server still needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadCheckRequest {
    /// All files the uploader intends to publish.
    pub files: Vec<FileEntry>,
}

/// Response of `POST /api/gamedata/upload/check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadCheckResponse {
    /// Paths the server does not have (or has with a different hash) and
    /// therefore still needs uploaded.
    pub needed: Vec<String>,
}

/// Request of `POST /api/gamedata/upload/commit`: finalize the manifest after
/// all needed files have been uploaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadCommitRequest {
    /// FAF patch version these files correspond to.
    pub patch_version: String,
    /// Display name of the uploading player.
    pub uploader: String,
    /// Complete file list for the new manifest (not only newly uploaded ones).
    pub files: Vec<FileEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip_json() {
        let manifest = Manifest {
            patch_version: "3825".to_string(),
            uploader: "tester".to_string(),
            generated_at: Utc::now(),
            files: vec![FileEntry {
                path: "faf.scd".to_string(),
                size: 42,
                sha256: "ab".repeat(32),
            }],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.patch_version, "3825");
        assert_eq!(back.files.len(), 1);
        assert_eq!(back.total_size(), 42);
        assert_eq!(back.summary().file_count, 1);
    }
}
