use std::path::{Path, PathBuf};

use async_trait::async_trait;
use extism::{CompiledPlugin, Manifest, Plugin, PluginBuilder, Wasm};
use kosong::tooling::CallableTool;
use serde_json::Value;

use crate::tools::plugin::manifest::{PluginManifest, PluginMetadata, default_schema};

/// A known export of the Brain WASM plugin ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginExport {
    /// Execute the tool with a JSON payload.
    Execute,
    /// Register one or more tools (modern ABI).
    RegisterTools,
    /// Legacy metadata export.
    ToolMetadata,
}

impl PluginExport {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginExport::Execute => "execute",
            PluginExport::RegisterTools => "register_tools",
            PluginExport::ToolMetadata => "tool_metadata",
        }
    }
}

impl std::fmt::Display for PluginExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A tool backed by a WebAssembly plugin using the Extism runtime.
#[derive(Clone)]
pub struct WasmPluginTool {
    name: String,
    description: String,
    schema: Value,
    compiled: CompiledPlugin,
}

#[async_trait]
impl CallableTool for WasmPluginTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.schema.clone()
    }

    async fn call_raw(&self, arguments: Value) -> kosong::tooling::ToolReturnValue {
        let input = serde_json::json!({
            "tool": self.name,
            "arguments": arguments,
        })
        .to_string();

        let compiled = self.compiled.clone();

        let output = match tokio::task::spawn_blocking(move || {
            let mut plugin = match Plugin::new_from_compiled(&compiled) {
                Ok(p) => p,
                Err(e) => return Err(format!("Failed to instantiate plugin: {}", e)),
            };

            plugin
                .call::<&str, &str>(PluginExport::Execute.as_str(), &input)
                .map_err(|e| format!("Plugin execution error: {}", e))
                .map(|s| s.to_string())
        })
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return kosong::tooling::ToolReturnValue::error(e),
            Err(e) => {
                return kosong::tooling::ToolReturnValue::error(format!(
                    "Plugin task panicked: {}",
                    e
                ));
            }
        };

        kosong::tooling::ToolReturnValue::ok(output)
    }
}

unsafe impl Send for WasmPluginTool {}
unsafe impl Sync for WasmPluginTool {}

/// Discover and load all WASM plugins from the given directory.
///
/// Scans for `.wasm` files and attempts to load each one as a plugin tool.
/// Failures are logged as warnings and skipped.
pub fn discover_plugins(plugins_dir: &Path) -> Vec<Box<dyn kosong::tooling::CallableTool>> {
    let mut tools = Vec::new();

    if !plugins_dir.is_dir() {
        return tools;
    }

    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(_) => return tools,
    };

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        if path.extension() != Some(std::ffi::OsStr::new("wasm")) {
            continue;
        }

        match load_tool_from_wasm(&path) {
            Ok(tool) => {
                tracing::info!("Loaded WASM plugin: {}", tool.name());
                tools.push(tool);
            }
            Err(e) => {
                tracing::warn!("Failed to load WASM plugin '{}': {}", path.display(), e);
            }
        }
    }

    tools
}

fn build_extism_manifest(wasm_bytes: &[u8], plugin_manifest: Option<&PluginManifest>) -> Manifest {
    let mut manifest = Manifest::new([Wasm::data(wasm_bytes.to_vec())]);

    if let Some(pm) = plugin_manifest {
        if let Some(ref hosts) = pm.allowed_hosts {
            manifest = manifest.with_allowed_hosts(hosts.iter().cloned());
        } else {
            manifest = manifest.disallow_all_hosts();
        }

        if let Some(ref paths) = pm.allowed_paths {
            manifest =
                manifest.with_allowed_paths(paths.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        if let Some(pages) = pm.max_memory_pages {
            manifest = manifest.with_memory_max(pages);
        }

        if let Some(ms) = pm.timeout_ms {
            manifest = manifest.with_timeout(std::time::Duration::from_millis(ms));
        }
    } else {
        manifest = manifest.disallow_all_hosts();
    }

    manifest
}

fn load_metadata_from_register_tools(
    compiled: &CompiledPlugin,
    path: &Path,
) -> Result<PluginMetadata, String> {
    let mut plugin = Plugin::new_from_compiled(compiled)
        .map_err(|e| format!("Failed to instantiate plugin for register_tools: {}", e))?;

    let export = PluginExport::RegisterTools;
    if !plugin.function_exists(export.as_str()) {
        return Err(format!("Plugin does not export '{}'", export));
    }

    let json = plugin
        .call::<&str, &str>(export.as_str(), "")
        .map_err(|e| format!("{} call failed: {}", export, e))?;

    let defs: Vec<crate::tools::plugin::manifest::ToolDef> =
        serde_json::from_str(json).map_err(|e| {
            format!(
                "Invalid register_tools JSON from '{}': {}",
                path.display(),
                e
            )
        })?;

    let first = defs.into_iter().next().ok_or_else(|| {
        format!(
            "register_tools returned empty list for '{}'",
            path.display()
        )
    })?;

    Ok(PluginMetadata {
        name: first.name,
        description: first.description,
        schema: first.parameters,
    })
}

fn load_metadata_from_plugin(
    compiled: &CompiledPlugin,
    path: &Path,
) -> Result<PluginMetadata, String> {
    let mut plugin = Plugin::new_from_compiled(compiled)
        .map_err(|e| format!("Failed to instantiate plugin for metadata: {}", e))?;

    let export = PluginExport::ToolMetadata;
    if !plugin.function_exists(export.as_str()) {
        return Err(format!("Plugin does not export '{}'", export));
    }

    let json = plugin
        .call::<&str, &str>(export.as_str(), "")
        .map_err(|e| format!("{} call failed: {}", export, e))?;

    serde_json::from_str(json).map_err(|e| {
        format!(
            "Invalid tool_metadata JSON from '{}': {}",
            path.display(),
            e
        )
    })
}

fn fallback_metadata(path: &Path) -> PluginMetadata {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    PluginMetadata {
        name: name.clone(),
        description: format!("WASM plugin tool: {}", name),
        schema: default_schema(),
    }
}

/// Minimal metadata about a loaded WASM plugin tool, suitable for UIs and status output.
#[derive(Debug, Clone)]
pub struct WasmToolInfo {
    pub name: String,
    pub description: String,
}

/// Load a single WASM plugin file and return the tool it exposes.
///
/// This is the public entry point used by tooling such as the `qqbot` CLI to
/// validate a `.wasm` file before registering it.
pub fn load_tool_from_wasm(path: &Path) -> Result<Box<dyn CallableTool>, String> {
    let wasm_bytes = std::fs::read(path).map_err(|e| format!("Failed to read WASM file: {}", e))?;

    let manifest_path = path.with_extension("json");
    let plugin_manifest: Option<PluginManifest> = if manifest_path.is_file() {
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Failed to read manifest: {}", e))?;
        Some(serde_json::from_str(&text).map_err(|e| format!("Invalid manifest JSON: {}", e))?)
    } else {
        None
    };

    let extism_manifest = build_extism_manifest(&wasm_bytes, plugin_manifest.as_ref());

    let compiled = PluginBuilder::new(extism_manifest)
        .with_wasi(true)
        .compile()
        .map_err(|e| format!("Failed to compile WASM plugin: {}", e))?;

    let metadata = if let Some(pm) = plugin_manifest {
        pm.into_metadata()
    } else {
        // Prefer the Brain tool ABI; fall back to the older `tool_metadata` export.
        load_metadata_from_register_tools(&compiled, path)
            .or_else(|_| load_metadata_from_plugin(&compiled, path))
            .unwrap_or_else(|_| fallback_metadata(path))
    };

    Ok(Box::new(WasmPluginTool {
        name: metadata.name,
        description: metadata.description,
        schema: metadata.schema,
        compiled,
    }))
}

/// Inspect a WASM plugin file and return its tool metadata without keeping the
/// compiled plugin alive.
pub fn inspect_wasm_plugin(path: &Path) -> Result<WasmToolInfo, String> {
    let tool = load_tool_from_wasm(path)?;
    Ok(WasmToolInfo {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
    })
}

/// Discover and return metadata for every loadable WASM plugin in a directory.
///
/// Invalid plugins are skipped (with tracing warnings) rather than failing the
/// whole scan. This is useful for status output and quick inventory.
pub fn discover_plugin_infos(plugins_dir: &Path) -> Vec<WasmToolInfo> {
    discover_plugins(plugins_dir)
        .into_iter()
        .map(|tool| WasmToolInfo {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
        })
        .collect()
}

/// Default plugins directory path (`~/.kimi/plugins`).
pub fn default_plugins_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".kimi/plugins"))
}

/// A [`ToolSource`] that discovers Extism `.wasm` plugins in a directory.
#[derive(Debug, Clone)]
pub struct ExtismPluginSource {
    plugins_dir: PathBuf,
}

impl ExtismPluginSource {
    /// Create a source that scans `plugins_dir` for `.wasm` files.
    pub fn new(plugins_dir: impl Into<PathBuf>) -> Self {
        Self {
            plugins_dir: plugins_dir.into(),
        }
    }
}

impl crate::core::registry::ToolSource for ExtismPluginSource {
    fn name(&self) -> &str {
        "extism-plugins"
    }

    fn load_tools(&self) -> Vec<Box<dyn kosong::tooling::CallableTool>> {
        discover_plugins(&self.plugins_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qqbot_plugins_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("data")
            .join("qqbot-data")
            .join("plugins")
    }

    #[test]
    fn test_discover_summary_plugin() {
        let dir = qqbot_plugins_dir();
        if !dir.join("summary.wasm").exists() {
            eprintln!("Skipping test: summary.wasm not found at {}", dir.display());
            return;
        }

        let tools = discover_plugins(&dir);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"summary_format_conversation"),
            "expected summary_format_conversation plugin tool, got {:?}",
            names
        );
    }

    #[tokio::test]
    async fn test_summary_plugin_tool_execution() {
        let dir = qqbot_plugins_dir();
        let wasm_path = dir.join("summary.wasm");
        if !wasm_path.exists() {
            eprintln!("Skipping test: summary.wasm not found");
            return;
        }

        let tools = discover_plugins(&dir);
        let tool = tools
            .into_iter()
            .find(|t| t.name() == "summary_format_conversation")
            .expect("summary plugin tool should be loaded");

        let args = serde_json::json!({
            "messages": "123: hello\n456: world",
            "style": "bullet"
        });

        let result = tool.call_raw(args).await;
        assert!(!result.is_error, "tool should succeed: {:?}", result);
        let output = result.output.unwrap().as_str().unwrap().to_string();
        assert!(output.contains("123: hello"));
        assert!(output.contains("456: world"));
    }
}
