mod discovery;
pub use discovery::{WasmPluginTool, default_plugins_dir, discover_plugins};

mod manifest;
pub use manifest::PluginManifest;
