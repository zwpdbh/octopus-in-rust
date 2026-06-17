use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Information about a WASM plugin crate in the `plugins/` directory.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Directory name, e.g. `faf-units`.
    pub dir_name: String,
    /// Package name from `Cargo.toml`, e.g. `faf-units-plugin`.
    pub package_name: String,
    /// Absolute path to the plugin directory.
    #[allow(dead_code)]
    pub path: PathBuf,
}

/// Scan `<root>/plugins` for plugin crates and read each package name from its
/// `Cargo.toml`. This avoids hard-coding plugin names in build scripts.
pub fn discover(root: &Path) -> Result<Vec<Plugin>> {
    let plugins_dir = root.join("plugins");
    if !plugins_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(&plugins_dir)
        .with_context(|| format!("failed to read {}", plugins_dir.display()))?
    {
        let entry = entry.with_context(|| "failed to read plugins directory entry")?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest = path.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }

        let contents = std::fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let package_name = parse_package_name(&contents)
            .with_context(|| format!("invalid Cargo.toml: {}", manifest.display()))?;

        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        plugins.push(Plugin {
            dir_name,
            package_name,
            path,
        });
    }

    plugins.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    Ok(plugins)
}

/// Convert a Cargo package name to the wasm file stem Cargo produces.
///
/// Example: `faf-units-plugin` → `faf_units_plugin`
pub fn wasm_stem(package_name: &str) -> String {
    package_name.replace('-', "_")
}

fn parse_package_name(manifest: &str) -> Result<String> {
    let doc: toml::Value = manifest
        .parse()
        .context("failed to parse Cargo.toml as TOML")?;

    let name = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .context("missing package.name in Cargo.toml")?;

    Ok(name.to_string())
}
