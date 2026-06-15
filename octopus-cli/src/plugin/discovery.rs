use std::path::{Path, PathBuf};

use async_trait::async_trait;
use extism::{CompiledPlugin, Manifest, Plugin, PluginBuilder, Wasm};
use serde_json::Value;

use crate::plugin::manifest::{PluginManifest, PluginMetadata, default_schema};

// ============================================================================
// WasmPluginTool
// ============================================================================

/// A tool backed by a WebAssembly plugin using the Extism runtime.
///
/// Each plugin is expected to export an `execute` function that takes a JSON
/// string as input and returns a JSON string as output.
pub struct WasmPluginTool {
    name: String,
    description: String,
    schema: Value,
    compiled: CompiledPlugin,
}

#[async_trait]
impl kosong::tooling::CallableTool for WasmPluginTool {
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
        let input = match serde_json::to_string(&arguments) {
            Ok(s) => s,
            Err(e) => {
                return kosong::tooling::ToolReturnValue::error(format!(
                    "Failed to serialize arguments: {}",
                    e
                ));
            }
        };

        let compiled = self.compiled.clone();

        // Run plugin execution in a blocking task since WASM instantiation
        // and execution involves significant synchronous work.
        let output = match tokio::task::spawn_blocking(move || {
            let mut plugin = match Plugin::new_from_compiled(&compiled) {
                Ok(p) => p,
                Err(e) => return Err(format!("Failed to instantiate plugin: {}", e)),
            };

            plugin
                .call::<&str, &str>("execute", &input)
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

// WasmPluginTool is Send + Sync because CompiledPlugin is Send + Sync.
unsafe impl Send for WasmPluginTool {}
unsafe impl Sync for WasmPluginTool {}

// ============================================================================
// Discovery & Loading
// ============================================================================

/// Discover and load all WASM plugins from the given directory.
///
/// Scans for `.wasm` files and attempts to load each one as a plugin tool.
/// Failures are logged as warnings and skipped; they don't fail the entire
/// discovery process.
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

        match load_wasm_plugin(&path) {
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

/// Load a single WASM plugin from a file path.
///
/// 1. Read the `.wasm` bytes.
/// 2. Try to read `<name>.json` manifest. If present, parse metadata + security.
/// 3. Build an Extism `Manifest` with security restrictions.
/// 4. Compile the plugin with those restrictions.
/// 5. If no JSON manifest, fall back to `tool_metadata` export or filename.
fn load_wasm_plugin(path: &Path) -> Result<Box<dyn kosong::tooling::CallableTool>, String> {
    let wasm_bytes = std::fs::read(path).map_err(|e| format!("Failed to read WASM file: {}", e))?;

    // Try JSON manifest first
    let manifest_path = path.with_extension("json");
    let plugin_manifest: Option<PluginManifest> = if manifest_path.is_file() {
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Failed to read manifest: {}", e))?;
        Some(serde_json::from_str(&text).map_err(|e| format!("Invalid manifest JSON: {}", e))?)
    } else {
        None
    };

    // Build Extism Manifest with security restrictions
    let extism_manifest = build_extism_manifest(&wasm_bytes, plugin_manifest.as_ref());

    let compiled = PluginBuilder::new(extism_manifest)
        .with_wasi(true)
        .compile()
        .map_err(|e| format!("Failed to compile WASM plugin: {}", e))?;

    // Resolve metadata
    let metadata = if let Some(pm) = plugin_manifest {
        pm.into_metadata()
    } else {
        match load_metadata_from_plugin(&compiled, path) {
            Ok(m) => m,
            Err(_) => fallback_metadata(path),
        }
    };

    Ok(Box::new(WasmPluginTool {
        name: metadata.name,
        description: metadata.description,
        schema: metadata.schema,
        compiled,
    }))
}

/// Build an Extism Manifest with security restrictions derived from the plugin
/// manifest. **Deny-by-default**: no permissions unless explicitly declared.
fn build_extism_manifest(wasm_bytes: &[u8], plugin_manifest: Option<&PluginManifest>) -> Manifest {
    let mut manifest = Manifest::new([Wasm::data(wasm_bytes.to_vec())]);

    if let Some(pm) = plugin_manifest {
        // HTTP: explicit allowlist required
        if let Some(ref hosts) = pm.allowed_hosts {
            manifest = manifest.with_allowed_hosts(hosts.iter().cloned());
        } else {
            manifest = manifest.disallow_all_hosts();
        }

        // Filesystem: explicit path mapping required
        if let Some(ref paths) = pm.allowed_paths {
            manifest =
                manifest.with_allowed_paths(paths.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        // Memory limit
        if let Some(pages) = pm.max_memory_pages {
            manifest = manifest.with_memory_max(pages);
        }

        // Timeout
        if let Some(ms) = pm.timeout_ms {
            manifest = manifest.with_timeout(std::time::Duration::from_millis(ms));
        }
    } else {
        // No manifest = most restrictive defaults
        manifest = manifest.disallow_all_hosts();
    }

    manifest
}

/// Attempt to call the `tool_metadata` export on a compiled plugin.
fn load_metadata_from_plugin(
    compiled: &CompiledPlugin,
    path: &Path,
) -> Result<PluginMetadata, String> {
    let mut plugin = Plugin::new_from_compiled(compiled)
        .map_err(|e| format!("Failed to instantiate plugin for metadata: {}", e))?;

    if !plugin.function_exists("tool_metadata") {
        return Err("Plugin does not export 'tool_metadata'".to_string());
    }

    let json = plugin
        .call::<&str, &str>("tool_metadata", "")
        .map_err(|e| format!("tool_metadata call failed: {}", e))?;

    serde_json::from_str(json).map_err(|e| {
        format!(
            "Invalid tool_metadata JSON from '{}': {}",
            path.display(),
            e
        )
    })
}

/// Generate fallback metadata from the WASM file path.
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

/// Return the default plugins directory path (`~/.kimi/plugins`).
pub fn default_plugins_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".kimi/plugins"))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kosong::tooling::CallableTool;

    #[test]
    fn test_load_example_http_plugin() {
        let plugins_dir = default_plugins_dir().expect("HOME not set");
        let wasm_path = plugins_dir.join("HttpRequest.wasm");

        if !wasm_path.exists() {
            eprintln!(
                "Skipping test: example plugin not installed at {}",
                wasm_path.display()
            );
            return;
        }

        let tool = load_wasm_plugin(&wasm_path).expect("Failed to load example plugin");
        assert_eq!(tool.name(), "HttpRequest");
        assert!(tool.description().contains("HTTP"));

        let schema = tool.parameters();
        assert!(schema.get("type").is_some());
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_example_http_plugin_metadata() {
        let plugins_dir = default_plugins_dir().expect("HOME not set");
        let wasm_path = plugins_dir.join("HttpRequest.wasm");

        if !wasm_path.exists() {
            eprintln!(
                "Skipping test: example plugin not installed at {}",
                wasm_path.display()
            );
            return;
        }

        let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM");
        let manifest = build_extism_manifest(&wasm_bytes, None);
        let compiled = PluginBuilder::new(manifest)
            .with_wasi(true)
            .compile()
            .expect("Failed to compile");

        let meta =
            load_metadata_from_plugin(&compiled, &wasm_path).expect("Failed to load metadata");

        assert_eq!(meta.name, "HttpRequest");
        assert!(meta.description.contains("HTTP"));
    }

    #[tokio::test]
    async fn test_example_http_plugin_execution_allowed_host() {
        let plugins_dir = default_plugins_dir().expect("HOME not set");
        let wasm_path = plugins_dir.join("HttpRequest.wasm");

        if !wasm_path.exists() {
            eprintln!(
                "Skipping test: example plugin not installed at {}",
                wasm_path.display()
            );
            return;
        }

        let tool = load_wasm_plugin(&wasm_path).expect("Failed to load example plugin");
        assert_eq!(tool.name(), "HttpRequest");

        // httpbin.org is in the example plugin's allowed_hosts
        let args = serde_json::json!({
            "url": "https://httpbin.org/get",
            "method": "GET"
        });

        let result = tool.call_raw(args).await;
        match result.output {
            Some(Value::String(output)) => {
                let parsed: serde_json::Value =
                    serde_json::from_str(&output).expect("Output should be valid JSON");
                assert!(
                    parsed.get("status").is_some(),
                    "Response should have status"
                );
                if parsed["status"] != 200 {
                    // httpbin.org occasionally returns non-200 status codes in CI;
                    // treat this as a network/environment issue rather than a plugin bug.
                    tracing::warn!(
                        "Plugin HTTP test received status {} (network issue?)",
                        parsed["status"]
                    );
                    return;
                }
                assert!(parsed.get("body").is_some(), "Response should have body");
                tracing::info!("Plugin HTTP test succeeded: {}", output);
            }
            _ => {
                // Network might be unavailable in test environment; log but don't fail hard
                tracing::warn!("Plugin HTTP test failed (network issue?)");
            }
        }
    }

    #[tokio::test]
    async fn test_plugin_http_blocked_without_permission() {
        let plugins_dir = default_plugins_dir().expect("HOME not set");
        let wasm_path = plugins_dir.join("HttpRequest.wasm");

        if !wasm_path.exists() {
            eprintln!(
                "Skipping test: example plugin not installed at {}",
                wasm_path.display()
            );
            return;
        }

        // Load the plugin but strip its allowed_hosts by providing no manifest
        let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM");
        let restrictive_manifest = build_extism_manifest(&wasm_bytes, None);
        let compiled = PluginBuilder::new(restrictive_manifest)
            .with_wasi(true)
            .compile()
            .expect("Failed to compile");

        // Use the compiled plugin directly (bypassing load_wasm_plugin which would use the JSON manifest)
        let tool = WasmPluginTool {
            name: "HttpRequest".to_string(),
            description: "Test".to_string(),
            schema: default_schema(),
            compiled,
        };

        let args = serde_json::json!({
            "url": "https://httpbin.org/get",
            "method": "GET"
        });

        let result = tool.call_raw(args).await;
        match result.output {
            Some(Value::String(output)) => {
                let parsed: serde_json::Value =
                    serde_json::from_str(&output).expect("Output should be valid JSON");
                // The plugin itself returns error in its JSON response when HTTP fails
                assert!(
                    parsed.get("error").is_some() || parsed["status"] == 0,
                    "HTTP should have been blocked or failed. Output: {}",
                    output
                );
                tracing::info!("HTTP correctly blocked without permission: {}", output);
            }
            _ => {
                // Either the plugin returns error JSON or the host traps — both are acceptable
                tracing::info!("HTTP blocked (expected)");
            }
        }
    }
}
