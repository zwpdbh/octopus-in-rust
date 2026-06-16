use crate::plugins;
use crate::service::run_dir;
use anyhow::Result;
use std::path::Path;

/// Set (or overwrite) the per-group system prompt.
pub fn set_prompt(data_dir: &Path, group_id: i64, prompt: &str) -> Result<()> {
    let mut profile = load_or_default(data_dir, group_id);
    profile.system_prompt = Some(prompt.to_string());
    save_and_reload(data_dir, group_id, &profile)
}

/// Enable a plugin for a specific group.
pub fn enable_plugin(data_dir: &Path, group_id: i64, plugin_name: &str) -> Result<()> {
    ensure_plugin_installed(data_dir, plugin_name)?;
    let mut profile = load_or_default(data_dir, group_id);
    profile.enable_plugin(plugin_name);
    save_and_reload(data_dir, group_id, &profile)
}

/// Disable a plugin for a specific group.
pub fn disable_plugin(data_dir: &Path, group_id: i64, plugin_name: &str) -> Result<()> {
    let mut profile = load_or_default(data_dir, group_id);
    profile.disable_plugin(plugin_name);
    save_and_reload(data_dir, group_id, &profile)
}

/// Show a group's effective profile and which plugins it would load.
pub fn show(data_dir: &Path, group_id: i64) -> Result<()> {
    let profile = load_or_default(data_dir, group_id);
    let installed = installed_plugin_names(data_dir);
    let allowed = profile.filter_plugins(installed.iter().map(|s| s.as_str()));

    println!("Group: {group_id}");
    println!();
    println!("System prompt:");
    if let Some(prompt) = &profile.system_prompt {
        println!("  {prompt}");
    } else {
        println!("  (using global system prompt from config.toml)");
    }
    println!();
    println!("Enabled plugins:");
    match &profile.enabled_plugins {
        Some(list) if !list.is_empty() => {
            for name in list {
                println!("  {name}");
            }
        }
        _ => println!("  (all installed plugins, except disabled ones)"),
    }
    println!();
    println!("Disabled plugins:");
    if profile.disabled_plugins.is_empty() {
        println!("  (none)");
    } else {
        for name in &profile.disabled_plugins {
            println!("  {name}");
        }
    }
    println!();
    println!("Plugins that would be loaded:");
    if allowed.is_empty() {
        println!("  (none)");
    } else {
        for name in allowed {
            println!("  {name}");
        }
    }
    Ok(())
}

fn load_or_default(data_dir: &Path, group_id: i64) -> qqbot_config::GroupProfile {
    match qqbot_config::GroupProfile::load(data_dir, group_id) {
        Ok(Some(p)) => p,
        Ok(None) => qqbot_config::GroupProfile::default(),
        Err(e) => {
            eprintln!("Warning: could not load group profile for {group_id}: {e}; using defaults");
            qqbot_config::GroupProfile::default()
        }
    }
}

fn save_and_reload(
    data_dir: &Path,
    group_id: i64,
    profile: &qqbot_config::GroupProfile,
) -> Result<()> {
    qqbot_config::GroupProfile::save(data_dir, group_id, profile)?;
    println!("Updated group profile for {group_id}");

    let pid_file = run_dir(data_dir).join("qqbot-core.pid");
    if pid_file.exists() {
        // Spawn a tiny runtime just to send SIGHUP, matching the existing plugin helpers.
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(plugins::reload(data_dir))?;
    } else {
        println!("qqbot-core is not running; profile will take effect on next start");
    }
    Ok(())
}

fn ensure_plugin_installed(data_dir: &Path, plugin_name: &str) -> Result<()> {
    let installed = installed_plugin_names(data_dir);
    if !installed.iter().any(|n| n == plugin_name) {
        anyhow::bail!(
            "plugin '{plugin_name}' is not installed in {}. Register it first with `qqbot tools register <path>`.",
            plugins::plugin_dir(data_dir).display()
        );
    }
    Ok(())
}

fn installed_plugin_names(data_dir: &Path) -> Vec<String> {
    let dir = plugins::plugin_dir(data_dir);
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names
}
