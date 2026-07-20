//! Helpers for loading the default FAF unit database shipped with the workspace.

use std::path::PathBuf;

use faf_units::DataIndex;

/// Return the path to the default `faf_units.json` file shipped with the
/// workspace.
///
/// The exact relative location depends on whether the caller is a crate or an
/// app, so this helper encodes the convention once.
pub fn default_units_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/faf-units/data/faf_units.json")
}

/// Load the raw `faf-units` index from the default JSON file.
pub fn load_default_data_index() -> anyhow::Result<DataIndex> {
    let path = default_units_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read units file {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("failed to parse units file {}: {e}", path.display()))
}
