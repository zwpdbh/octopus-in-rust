use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cargo;

/// Run a faf-ml-specific command (the FAF unit-detection ML platform).
pub fn run(command: &str, rest: &[String]) -> Result<()> {
    match command {
        "backend" => run_backend(),
        "frontend" => run_frontend(),
        "build-web" => build_web(rest),
        "datagen" => run_datagen(rest),
        "import" => import_datagen(rest),
        "help" | "-h" | "--help" => {
            crate::args::print_faf_ml_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown faf-ml command '{}'", other);
            crate::args::print_faf_ml_help();
            std::process::exit(1);
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Start the Axum backend (serves the API on :3100 and, if built, the
/// release web UI at the same port).
fn run_backend() -> Result<()> {
    let mut cmd = cargo::command();
    cmd.args(["run", "--package", "faf-ml-server"]);

    if std::env::var_os("RUST_LOG").is_none() {
        cmd.env("RUST_LOG", "info,faf_ml_server=debug,tower_http=info");
    }

    println!("Starting faf-ml backend on http://localhost:3100 ...");
    let mut child = cmd.spawn().context("failed to spawn faf-ml-server")?;
    println!("Server PID: {}", child.id());

    let status = child.wait().context("failed to wait for faf-ml-server")?;
    if !status.success() {
        anyhow::bail!("faf-ml-server exited with status: {status}");
    }
    Ok(())
}

/// Start the Dioxus dev server with hot reload (debug builds call the
/// backend on localhost:3100).
fn run_frontend() -> Result<()> {
    let mut cmd = std::process::Command::new("dx");
    cmd.args(["serve", "--platform", "web", "--port", "8081"]);
    cmd.current_dir("apps/faf-ml-web");

    println!("Starting faf-ml frontend (hot reload)...");
    let status = cmd.status().context("failed to run dx serve")?;
    if !status.success() {
        anyhow::bail!("dx serve exited with status: {status}");
    }
    Ok(())
}

/// Build the web UI. Release is the default because the backend serves the
/// release bundle (target/dx/faf-ml-web/release/web/public).
fn build_web(rest: &[String]) -> Result<()> {
    let debug = rest.iter().any(|a| a == "--debug");
    let mut cmd = std::process::Command::new("dx");
    cmd.args(["build", "--platform", "web"]);
    if !debug {
        cmd.arg("--release");
    }
    cmd.current_dir("apps/faf-ml-web");

    println!(
        "Building faf-ml-web ({})...",
        if debug { "debug" } else { "release" }
    );
    let status = cmd.status().context("failed to run dx build")?;
    if !status.success() {
        anyhow::bail!("dx build exited with status: {status}");
    }
    Ok(())
}

/// Generate synthetic training data (passthrough args to faf-datagen):
///   cargo xtask faf-ml datagen --count 1000 --previews 20
fn run_datagen(rest: &[String]) -> Result<()> {
    let mut cmd = cargo::command();
    cmd.args(["run", "--release", "--package", "faf-datagen", "--"]);
    // Allow an optional leading `--` separator.
    let args: Vec<&str> = rest
        .iter()
        .map(String::as_str)
        .skip_while(|a| *a == "--")
        .collect();
    cmd.args(&args);
    println!("Running faf-datagen {:?}...", args);
    cargo::run(&mut cmd).context("faf-datagen failed")?;
    Ok(())
}

/// Import a faf-datagen output directory into a RUNNING faf-ml-server
/// (default dir: data/faf-detect).
fn import_datagen(rest: &[String]) -> Result<()> {
    let dir = rest
        .iter()
        .find(|a| a.as_str() != "--")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("data/faf-detect"));
    let dir = dir
        .canonicalize()
        .with_context(|| format!("{dir:?} not found"))?;

    println!("Importing {dir:?} into faf-ml-server on localhost:3100...");
    let status = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "-X",
            "POST",
            "http://localhost:3100/api/import/datagen",
            "-H",
            "Content-Type: application/json",
            "-d",
            &format!("{{\"dir\":\"{}\"}}", dir.display()),
        ])
        .status()
        .context("failed to run curl (is the backend running? `cargo xtask faf-ml backend`)")?;
    if !status.success() {
        anyhow::bail!("import request failed — is `cargo xtask faf-ml backend` running?");
    }
    println!();
    Ok(())
}
