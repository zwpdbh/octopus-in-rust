//! Shared manifest and API types for the FAF gamedata sync service.
//!
//! Both `fafcn-server` (HTTP API) and `fafcn-sync` (CLI client) depend on this
//! crate so the manifest format and upload protocol can never drift apart.

mod error;
mod hash;
mod manifest;
mod overlay;
mod path;

pub use error::{Error, Result};
pub use hash::{sha256_bytes, sha256_file};
pub use manifest::{
    FileEntry, Manifest, ManifestSummary, StatusResponse, UploadCheckRequest, UploadCheckResponse,
    UploadCommitRequest,
};
pub use overlay::{append_config, read_config, EmbeddedConfig};
pub use path::validate_relative_path;
