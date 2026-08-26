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
    /// Per-channel mirror state (gamedata, map-generator).
    pub channels: Vec<ChannelStatus>,
    /// Build tag of the sync client binary currently served, so the web page
    /// can show users which build they will download.
    #[serde(default)]
    pub client_tag: Option<String>,
    /// State of the server-side auto-updater that fetches official FAF
    /// patches. `None` when talking to an older server without the updater.
    #[serde(default)]
    pub updater: Option<UpdaterInfo>,
}

/// Which component the server-side auto-updater is working on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdaterComponent {
    /// The gamedata patch archives (default for payloads from older servers
    /// that predate the component field).
    #[default]
    Gamedata,
    /// The FAF client installer.
    FafClient,
}

/// State of the server-side auto-updater for official FAF patches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdaterState {
    /// Nothing happening right now.
    Idle,
    /// Querying the official FAF patch version.
    Checking,
    /// Downloading official files for `version`.
    Downloading {
        /// Which component is being downloaded.
        #[serde(default)]
        component: UpdaterComponent,
        /// Version being downloaded.
        version: String,
    },
}

/// Snapshot of the server-side auto-updater, returned by
/// `GET /api/gamedata/status` and `POST /api/gamedata/upstream/refresh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterInfo {
    /// What the updater is doing right now.
    pub state: UpdaterState,
    /// Latest official FAF patch version seen (from the last check).
    pub latest_official_version: Option<String>,
    /// Latest FAF client release version seen on GitHub (from the last
    /// check). `None` on older servers without the client auto-mirror.
    #[serde(default)]
    pub latest_client_version: Option<String>,
    /// When the official version was last checked.
    pub last_check_at: Option<DateTime<Utc>>,
    /// Why the last update attempt failed, if it did.
    pub last_error: Option<String>,
}

/// Mirror state of one sync channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatus {
    /// Channel id (e.g. "gamedata", "map-generator").
    pub name: String,
    /// Current channel contents; `None` when nothing has been uploaded yet.
    pub manifest: Option<ManifestSummary>,
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
