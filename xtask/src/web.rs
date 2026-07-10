use std::fs;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::cargo;
use crate::project;

/// Web-specific workflow commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebCommand {
    Build,
    Serve,
}

/// Profile string used by Cargo for the WASM target directory.
fn wasm_profile_str(release: bool) -> &'static str {
    if release {
        "release"
    } else {
        "debug"
    }
}

/// Run a web workflow command.
pub fn run(command: &WebCommand, release: bool, port: u16) -> Result<()> {
    match command {
        WebCommand::Build => build(release),
        WebCommand::Serve => serve(release, port),
    }
}

/// Build the Bevy app for the web and run `wasm-bindgen`.
pub fn build(release: bool) -> Result<()> {
    let root = project::root();

    // 1. Build the CLI binary for the WASM target without native-only deps.
    let mut cmd = cargo::command();
    cmd.args([
        "build",
        "--bin",
        "faf-sim",
        "--target",
        "wasm32-unknown-unknown",
        "--no-default-features",
        "--features",
        "web",
    ]);
    if release {
        cmd.arg("--release");
    }
    println!("Building WASM binary ({})...", wasm_profile_str(release));
    cargo::run(&mut cmd).context("WASM build failed")?;

    // 2. Generate the JS glue and WASM bundle with wasm-bindgen.
    let wasm_path = root
        .join("target")
        .join("wasm32-unknown-unknown")
        .join(wasm_profile_str(release))
        .join("faf-sim.wasm");

    if !wasm_path.exists() {
        bail!("expected WASM artifact at {}", wasm_path.display());
    }

    let out_dir = root.join("apps").join("faf-sim-cli").join("web");
    let mut bindgen = Command::new("wasm-bindgen");
    bindgen.args([
        "--out-dir",
        out_dir.to_str().context("invalid UTF-8 in out-dir path")?,
        "--out-name",
        "faf_sim",
        "--target",
        "web",
        "--no-typescript",
        wasm_path.to_str().context("invalid UTF-8 in wasm path")?,
    ]);
    println!("Running wasm-bindgen for {}...", wasm_path.display());
    let status = bindgen.status().context("failed to spawn wasm-bindgen")?;
    if !status.success() {
        bail!("wasm-bindgen failed (status: {status})");
    }

    // 3. Copy static assets so the WASM build can load icons.
    let assets_src = root.join("assets");
    let assets_dst = out_dir.join("assets");
    if assets_src.exists() {
        copy_dir_all(&assets_src, &assets_dst)
            .with_context(|| format!("copying assets to {}", assets_dst.display()))?;
        println!("Copied assets to {}", assets_dst.display());
    }

    println!("Web build ready at {}", out_dir.display());
    Ok(())
}

fn copy_dir_all(src: impl AsRef<std::path::Path>, dst: impl AsRef<std::path::Path>) -> Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Build the web bundle and then start the embedded Axum server.
pub fn serve(release: bool, port: u16) -> Result<()> {
    build(release)?;

    let port_str = port.to_string();
    let mut args = vec!["run"];
    if release {
        args.push("--release");
    }
    args.extend(["--bin", "faf-sim", "--", "serve", "--port", &port_str]);

    let mut cmd = cargo::command();
    cmd.args(args);

    println!("Starting web server on port {port}...");
    let status = cmd.status().context("failed to spawn server")?;
    if !status.success() {
        bail!("server exited with status: {status}");
    }
    Ok(())
}
