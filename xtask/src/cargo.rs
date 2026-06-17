use std::process::Command;

use anyhow::{Context, Result};

/// Return a `cargo` Command pre-filled with the `CARGO` environment variable
/// if present (so rustup toolchains work correctly).
pub fn command() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Run a cargo command and fail if it does not succeed.
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().context("failed to spawn cargo")?;
    if !status.success() {
        return Err(bail_from_status(cmd, status));
    }
    Ok(())
}

/// Run `cargo build` for the host target.
pub fn build_host(release: bool) -> Result<()> {
    let mut cmd = command();
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    println!("Building host binaries ({})...", crate::project::profile_str(release));
    run(&mut cmd).context("host build failed")?;
    Ok(())
}

/// Run `cargo build --target wasm32-unknown-unknown` for a plugin package.
pub fn build_plugin(package_name: &str, release: bool) -> Result<()> {
    let mut cmd = command();
    cmd.args([
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "-p",
        package_name,
    ]);
    if release {
        cmd.arg("--release");
    }
    println!(
        "Building plugin {} ({})...",
        package_name,
        crate::project::profile_str(release)
    );
    run(&mut cmd).with_context(|| format!("plugin build failed for {package_name}"))?;
    Ok(())
}

/// Run `cargo test --workspace`.
pub fn test_workspace() -> Result<()> {
    let mut cmd = command();
    cmd.args(["test", "--workspace"]);
    println!("Running workspace tests...");
    run(&mut cmd).context("tests failed")?;
    Ok(())
}

fn bail_from_status(cmd: &Command, status: std::process::ExitStatus) -> anyhow::Error {
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    let args_str = args.join(" ");
    anyhow::anyhow!("cargo command failed: {program} {args_str} (status: {status})")
}
