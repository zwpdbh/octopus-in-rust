use crate::paths;
use crate::service::run_dir;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const WASM_TARGET_DIR: &str = "target/wasm32-unknown-unknown/release";

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub available: bool,
    pub enabled: bool,
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
    let src = paths::project_root()
        .join(WASM_TARGET_DIR)
        .join(format!("{name}.wasm"));
    if !src.exists() {
        anyhow::bail!(
            "plugin '{name}' is not available; expected {}\nBuild it with: cargo build --release -p {name} --target wasm32-unknown-unknown",
            src.display()
        );
    }

    let dst = plugin_dir(data_dir).join(format!("{name}.wasm"));
    std::fs::create_dir_all(plugin_dir(data_dir))?;
    tokio::fs::copy(&src, &dst).await?;
    println!("Enabled plugin '{name}'");

    reload(data_dir).await?;
    Ok(())
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
    let dir = paths::project_root().join(WASM_TARGET_DIR);
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
        "summary" => "Summarize recent group chat messages",
        "example-http" => "Example HTTP-based plugin",
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
