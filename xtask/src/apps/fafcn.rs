use anyhow::{Context, Result};
use std::process::Stdio;

use crate::cargo;

/// Run a fafcn-specific command.
pub fn run(command: &str, _rest: &[String]) -> Result<()> {
    match command {
        "backend" => run_backend(),
        "frontend" => run_frontend(),
        "help" | "-h" | "--help" => {
            crate::args::print_fafcn_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown fafcn command '{}'", other);
            crate::args::print_fafcn_help();
            std::process::exit(1);
        }
    }
}

fn run_backend() -> Result<()> {
    let log_path = std::path::PathBuf::from("data/logs/fafcn-server.log");
    std::fs::create_dir_all(log_path.parent().unwrap())
        .with_context(|| format!("failed to create log directory {}", log_path.display()))?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open log file {}", log_path.display()))?;

    let mut cmd = cargo::command();
    cmd.args(["run", "--package", "fafcn-server"]);
    cmd.stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file));

    println!("Starting fafcn backend...");
    println!("Server log: {}", log_path.display());
    let mut child = cmd.spawn().context("failed to spawn fafcn-server")?;
    println!("Server PID: {}", child.id());
    let status = child.wait().context("failed to wait for fafcn-server")?;
    if !status.success() {
        anyhow::bail!("fafcn-server exited with status: {status}");
    }
    Ok(())
}

fn run_frontend() -> Result<()> {
    let mut cmd = std::process::Command::new("dx");
    cmd.args(["serve", "--platform", "web", "--port", "8080"]);
    cmd.current_dir("apps/fafcn-web");

    println!("Starting fafcn frontend...");
    let status = cmd.status().context("failed to run dx serve")?;
    if !status.success() {
        anyhow::bail!("dx serve exited with status: {status}");
    }
    Ok(())
}
