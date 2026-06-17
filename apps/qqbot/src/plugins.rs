use crate::paths;
use crate::service::run_dir;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn wasm_target_dir() -> PathBuf {
    let profile = if std::env::current_exe()
        .map(|e| e.to_string_lossy().contains("/debug/"))
        .unwrap_or(false)
    {
        "debug"
    } else {
        "release"
    };
    paths::project_root().join(format!("target/wasm32-unknown-unknown/{profile}"))
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub available: bool,
    pub enabled: bool,
}

/// Summary of a registered WASM plugin tool, as reported by the brain loader.
#[derive(Debug, Clone)]
pub struct RegisteredTool {
    pub name: String,
    pub description: String,
}

pub fn list(data_dir: &Path) -> Result<Vec<PluginInfo>> {
    let available = available_plugins()?;
    let enabled: BTreeSet<String> = enabled_plugins(data_dir)?;

    let mut all: BTreeSet<String> = available.iter().cloned().collect();
    all.extend(enabled.iter().cloned());

    let mut plugins: Vec<PluginInfo> = all
        .into_iter()
        .map(|name| PluginInfo {
            available: available.contains(&name),
            enabled: enabled.contains(&name),
            name,
        })
        .collect();
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

pub async fn enable(data_dir: &Path, name: &str) -> Result<()> {
    let src = wasm_target_dir().join(format!("{name}.wasm"));
    if !src.exists() {
        anyhow::bail!(
            "plugin '{name}' is not available; expected {}\nBuild it with: cargo build --release -p {name} --target wasm32-unknown-unknown",
            src.display()
        );
    }

    register(data_dir, &src).await?;
    Ok(())
}

/// Register a built `.wasm` plugin file from an explicit path.
///
/// The file is validated by the brain plugin loader, copied into the data
/// directory's plugin folder, and qqbot-core is signalled to reload if it is
/// currently running.
pub async fn register(data_dir: &Path, wasm_path: &Path) -> Result<String> {
    if !wasm_path.exists() {
        anyhow::bail!("WASM file not found: {}", wasm_path.display());
    }
    if wasm_path.extension().and_then(|e| e.to_str()) != Some("wasm") {
        anyhow::bail!("not a .wasm file: {}", wasm_path.display());
    }

    let name = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("invalid WASM file name")?
        .to_string();

    // Validate the plugin with the internal brain loader before installing it.
    let info = brain::tools::plugin::inspect_wasm_plugin(wasm_path).map_err(|e| {
        anyhow::anyhow!("failed to inspect WASM plugin {}: {e}", wasm_path.display())
    })?;

    let dst = plugin_dir(data_dir).join(format!("{name}.wasm"));
    let already_installed = dst.exists();
    std::fs::create_dir_all(plugin_dir(data_dir))?;
    tokio::fs::copy(wasm_path, &dst).await?;
    let action = if already_installed {
        "Updated"
    } else {
        "Installed"
    };
    println!("{action} plugin '{name}' (tool: {})", info.name);

    // Signal the running core only if there is a pid file. If the core is not
    // running the plugin will be picked up on the next start.
    let pid_file = run_dir(data_dir).join("qqbot-core.pid");
    if pid_file.exists() {
        reload(data_dir).await?;
    } else {
        println!("qqbot-core is not running; plugin will be loaded on next start");
    }

    Ok(name)
}

/// Uninstall a previously registered plugin by its file-stem name.
///
/// The `.wasm` file is removed from the plugin directory and the running core
/// is signalled to reload if it is active.
pub async fn unregister(data_dir: &Path, name: &str) -> Result<()> {
    let dst = plugin_dir(data_dir).join(format!("{name}.wasm"));
    if !dst.exists() {
        anyhow::bail!("plugin '{name}' is not installed");
    }
    tokio::fs::remove_file(&dst).await?;
    println!("Uninstalled plugin '{name}'");

    let pid_file = run_dir(data_dir).join("qqbot-core.pid");
    if pid_file.exists() {
        reload(data_dir).await?;
    } else {
        println!("qqbot-core is not running; change will take effect on next start");
    }
    Ok(())
}

/// List the tools currently registered in the plugin directory.
///
/// Only plugins that can be successfully loaded by the brain plugin loader are
/// returned; corrupt or incompatible files are skipped with a warning log.
pub fn list_registered(data_dir: &Path) -> Result<Vec<RegisteredTool>> {
    let dir = plugin_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    Ok(brain::tools::plugin::discover_plugin_infos(&dir)
        .into_iter()
        .map(|info| RegisteredTool {
            name: info.name,
            description: info.description,
        })
        .collect())
}

pub async fn disable(data_dir: &Path, name: &str) -> Result<()> {
    let dst = plugin_dir(data_dir).join(format!("{name}.wasm"));
    if !dst.exists() {
        anyhow::bail!("plugin '{name}' is not enabled");
    }
    tokio::fs::remove_file(&dst).await?;
    println!("Disabled plugin '{name}'");

    reload(data_dir).await?;
    Ok(())
}

pub async fn reload(data_dir: &Path) -> Result<()> {
    let pid_file = run_dir(data_dir).join("qqbot-core.pid");
    if !pid_file.exists() {
        anyhow::bail!("qqbot-core pid file not found; is the daemon running?");
    }
    let contents = tokio::fs::read_to_string(&pid_file).await?;
    let pid: i32 = contents.trim().parse().context("invalid pid file")?;

    send_sighup(pid).context("failed to send SIGHUP to qqbot-core")?;
    println!("Sent SIGHUP to qqbot-core (pid {pid}); plugins will reload.");
    Ok(())
}

pub fn available_plugins() -> Result<BTreeSet<String>> {
    let dir = wasm_target_dir();
    if !dir.exists() {
        return Ok(BTreeSet::new());
    }
    Ok(scan_wasm_names(&dir))
}

pub fn enabled_plugins(data_dir: &Path) -> Result<BTreeSet<String>> {
    let dir = plugin_dir(data_dir);
    if !dir.exists() {
        return Ok(BTreeSet::new());
    }
    Ok(scan_wasm_names(&dir))
}

fn scan_wasm_names(dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
    }
    names
}

pub fn plugin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

pub fn plugin_description(name: &str) -> &'static str {
    match name {
        "example-http" => "Example HTTP-based plugin",
        "faf_units_plugin" => "Query and compare FAF units",
        _ => "No description available",
    }
}

/// Reminder shown in the health-check message about how to talk to the bot.
pub fn help_reminder() -> &'static str {
    "Mention the bot with @<question> to chat."
}

#[cfg(unix)]
fn send_sighup(pid: i32) -> Result<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), Signal::SIGHUP)?;
    Ok(())
}

#[cfg(not(unix))]
fn send_sighup(_pid: i32) -> Result<()> {
    anyhow::bail!("plugin reload via SIGHUP is only supported on Unix")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_wasm_path() -> Option<PathBuf> {
        let from_data = paths::project_root().join("data/qqbot-data/plugins/faf_units_plugin.wasm");
        if from_data.exists() {
            return Some(from_data);
        }
        let from_target = paths::project_root()
            .join("target/wasm32-unknown-unknown/release/faf_units_plugin.wasm");
        if from_target.exists() {
            return Some(from_target);
        }
        None
    }

    #[tokio::test]
    async fn test_register_plugin_from_wasm() {
        let Some(src) = sample_wasm_path() else {
            eprintln!("Skipping test_register_plugin_from_wasm: faf_units_plugin.wasm not found");
            return;
        };

        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let wasm_path = data_dir.join("faf_units_plugin.wasm");
        tokio::fs::copy(&src, &wasm_path).await.unwrap();

        let name = register(data_dir, &wasm_path).await.unwrap();
        assert_eq!(name, "faf_units_plugin");

        let tools = list_registered(data_dir).unwrap();
        assert!(
            tools.iter().any(|t| t.name == "faf_units_search"),
            "expected faf_units_search in {:?}",
            tools
        );
    }

    #[tokio::test]
    async fn test_unregister_plugin() {
        let Some(src) = sample_wasm_path() else {
            eprintln!("Skipping test_unregister_plugin: faf_units_plugin.wasm not found");
            return;
        };

        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let wasm_path = data_dir.join("faf_units_plugin.wasm");
        tokio::fs::copy(&src, &wasm_path).await.unwrap();

        register(data_dir, &wasm_path).await.unwrap();
        assert!(plugin_dir(data_dir).join("faf_units_plugin.wasm").exists());

        unregister(data_dir, "faf_units_plugin").await.unwrap();
        assert!(!plugin_dir(data_dir).join("faf_units_plugin.wasm").exists());

        let tools = list_registered(data_dir).unwrap();
        assert!(tools.is_empty(), "after unregister no tools should remain");
    }

    #[test]
    fn test_list_registered_skips_invalid_files() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        std::fs::create_dir_all(plugin_dir(data_dir)).unwrap();
        std::fs::write(
            plugin_dir(data_dir).join("not-a-plugin.wasm"),
            b"invalid wasm bytes",
        )
        .unwrap();

        let tools = list_registered(data_dir).unwrap();
        assert!(
            tools.is_empty(),
            "invalid wasm should be skipped, got {:?}",
            tools
        );
    }
}
