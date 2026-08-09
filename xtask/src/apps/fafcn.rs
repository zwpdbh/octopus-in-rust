use std::path::Path;

use anyhow::{Context, Result};

use crate::cargo;

/// Plugins the fafcn-server backend expects to find on disk.
const REQUIRED_PLUGINS: &[&str] = &["data/qqbot-data/plugins/faf_units_plugin.wasm"];

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
    ensure_plugins()?;

    let mut cmd = cargo::command();
    cmd.args(["run", "--package", "fafcn-server"]);

    // Keep dependency noise (reqwest, hyper, extism, wasmtime, rustls) at info/warn
    // while still showing debug output from the application crates.  Respect any
    // RUST_LOG the user already has set.
    if std::env::var_os("RUST_LOG").is_none() {
        cmd.env(
            "RUST_LOG",
            "info,fafcn_server=debug,agent_core=debug,llm_provider=debug,reqwest=warn,hyper=warn,hyper_util=warn,rustls=warn,extism=warn,wasmtime=warn",
        );
    }

    println!("Starting fafcn backend...");
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

/// Verify that all WASM plugins required by the backend exist.
fn ensure_plugins() -> Result<()> {
    let mut missing = Vec::new();
    for path in REQUIRED_PLUGINS {
        if !Path::new(path).is_file() {
            missing.push(*path);
        }
    }

    if missing.is_empty() {
        return Ok(());
    }

    eprintln!("Missing WASM plugin(s) required by the fafcn-server backend:");
    for path in &missing {
        eprintln!("  - {path}");
    }
    eprintln!();
    eprintln!("Build and install them with:");
    eprintln!("  cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown");
    eprintln!("  mkdir -p data/qqbot-data/plugins");
    eprintln!(
        "  cp target/wasm32-unknown-unknown/release/faf_units_plugin.wasm data/qqbot-data/plugins/"
    );

    anyhow::bail!("missing required plugins");
}
