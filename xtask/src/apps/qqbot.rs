use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::cargo;
use crate::deploy;
use crate::plugins::{self, Plugin};
use crate::project;

/// Run a qqbot-specific command.
pub fn run(command: &str, rest: &[String]) -> Result<()> {
    match command {
        "build" => build(false),
        "release" => build(true),
        "start" => start(false),
        "start-release" => start(true),
        "stop" => run_qqbot(["stop"].iter().map(|s| s.to_string()), project::profile()),
        "restart" => restart(false),
        "restart-release" => restart(true),
        "status" => run_qqbot(["status"].iter().map(|s| s.to_string()), project::profile()),
        "health" => run_qqbot(["health"].iter().map(|s| s.to_string()), project::profile()),
        "logs" => {
            let mut cmd = vec!["logs".to_string()];
            cmd.extend(rest.iter().cloned());
            run_qqbot(cmd, project::profile())
        }
        "doctor" => run_qqbot(["doctor"].iter().map(|s| s.to_string()), project::profile()),
        "deploy" => deploy::run(),
        "remote-status" => deploy::remote_cmd("status"),
        "remote-health" => deploy::remote_cmd("health"),
        "remote-logs" => deploy::remote_logs(rest),
        "remote-start" => deploy::remote_service_cmd("start"),
        "remote-stop" => deploy::remote_service_cmd("stop"),
        "remote-restart" => deploy::remote_service_cmd("restart"),
        "remote-doctor" => deploy::remote_cmd("doctor"),
        "remote-destroy" => deploy::remote_destroy(),
        "help" => print_help(),
        _ => print_help(),
    }
}

fn print_help() -> Result<()> {
    println!("xtask qqbot — development tasks for the QQ bot");
    println!();
    println!("Usage: cargo xtask qqbot <command> [args]");
    println!();
    println!("Commands:");
    println!("  build            Build host binaries + WASM plugins (debug)");
    println!("  release          Build host binaries + WASM plugins (release)");
    println!("  start            Build debug and start the daemon");
    println!("  start-release    Build release and start the daemon");
    println!("  stop             Stop the daemon");
    println!("  restart          Build debug and restart the daemon");
    println!("  restart-release  Build release and restart the daemon");
    println!("  status           Show qqbot status");
    println!("  health           Run qqbot health check");
    println!("  logs [args]      Show qqbot logs (e.g. 'cargo xtask qqbot logs core -n 50')");
    println!("  doctor           Run qqbot doctor");
    println!("  deploy           Build release and deploy to AliCloud ECS");
    println!("  remote-status    Show qqbot status on the remote host");
    println!("  remote-health    Run health check on the remote host");
    println!("  remote-logs      Show remote qqbot logs");
    println!("  remote-start     Start the remote qqbot systemd service");
    println!("  remote-stop      Stop the remote qqbot systemd service");
    println!("  remote-restart   Restart the remote qqbot systemd service");
    println!("  remote-doctor    Run doctor on the remote host");
    println!("  remote-destroy   Delete the AliCloud ECS instance");
    Ok(())
}

fn build(release: bool) -> Result<()> {
    cargo::build_host(release)?;

    let discovered = plugins::discover(&project::root())?;
    if discovered.is_empty() {
        println!("No plugins found in plugins/");
    } else {
        for plugin in &discovered {
            cargo::build_plugin(&plugin.package_name, release)?;
            install_plugin(plugin, release)?;
        }
    }

    println!("Build complete.");
    Ok(())
}

fn start(release: bool) -> Result<()> {
    build(release)?;
    println!("Starting daemon...");
    run_qqbot(
        ["start"].iter().map(|s| s.to_string()),
        project::profile_str(release),
    )
}

fn restart(release: bool) -> Result<()> {
    build(release)?;
    println!("Restarting daemon...");
    run_qqbot(
        ["restart"].iter().map(|s| s.to_string()),
        project::profile_str(release),
    )
}

/// Copy a freshly built plugin into the runtime plugin directory.
fn install_plugin(plugin: &Plugin, release: bool) -> Result<()> {
    let profile = project::profile_str(release);
    let stem = plugins::wasm_stem(&plugin.package_name);
    let src = project::root().join(format!(
        "target/wasm32-unknown-unknown/{profile}/{stem}.wasm"
    ));
    if !src.exists() {
        bail!("built plugin not found at {}", src.display());
    }

    let plugins_dir = project::data_dir()?.join("plugins");
    std::fs::create_dir_all(&plugins_dir)
        .with_context(|| format!("failed to create {}", plugins_dir.display()))?;

    let dst = plugins_dir.join(format!("{stem}.wasm"));
    std::fs::copy(&src, &dst)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dst.display()))?;
    println!("Installed plugin: {} → {}", src.display(), dst.display());
    Ok(())
}

/// Run the `qqbot` CLI binary for the given profile.
fn run_qqbot<I, S>(args: I, profile: &str) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let exe = qqbot_binary(profile)?;
    let status = Command::new(&exe)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {}", exe.display()))?;

    if !status.success() {
        bail!("{} exited with status {}", exe.display(), status);
    }
    Ok(())
}

fn qqbot_binary(profile: &str) -> Result<PathBuf> {
    let exe = project::root().join(format!("target/{profile}/qqbot"));
    if !exe.exists() {
        bail!(
            "qqbot binary not found at {}. Run `cargo xtask qqbot build` first.",
            exe.display()
        );
    }
    Ok(exe)
}
