use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Full plugin manifest including metadata and security settings.
///
/// Security model: **deny-by-default**. If `allowed_hosts` is not specified,
/// the plugin cannot make any HTTP requests. If `allowed_paths` is not
/// specified, the plugin has no filesystem access beyond WASI defaults.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    /// Tool name shown to the LLM.
    pub name: String,
    /// Tool description shown to the LLM.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    #[serde(default = "default_schema")]
    pub schema: Value,

    // --- Security settings (deny-by-default) ---
    /// Which hosts the plugin may access via HTTP.
    /// `None` or `Some([])` means **no HTTP access**.
    /// Wildcards are supported, e.g. `["*.github.com"]`.
    #[serde(default)]
    pub allowed_hosts: Option<Vec<String>>,

    /// Which host paths are mapped into the plugin's WASI filesystem.
    /// Key = host path, Value = guest path.
    #[serde(default)]
    pub allowed_paths: Option<BTreeMap<String, PathBuf>>,

    /// Maximum plugin execution time in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,

    /// Maximum WASM memory pages (1 page = 64 KiB).
    #[serde(default)]
    pub max_memory_pages: Option<u32>,
}

pub(crate) fn default_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {}
    })
}

impl PluginManifest {
    /// Extract just the metadata fields (name, description, schema).
    pub(crate) fn into_metadata(self) -> PluginMetadata {
        PluginMetadata {
            name: self.name,
            description: self.description,
            schema: self.schema,
        }
    }
}

/// Metadata describing a WASM plugin tool (subset of PluginManifest).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PluginMetadata {
    pub name: String,
    pub description: String,
    #[serde(default = "default_schema")]
    pub schema: Value,
}
