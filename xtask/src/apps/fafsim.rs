use anyhow::{bail, Context, Result};

use crate::cargo;
use crate::web::{self, WebCommand};

/// Run a faf-sim-specific command.
pub fn run(command: &str, rest: &[String]) -> Result<()> {
    match command {
        "run" => run_native(rest),
        "web" => run_web(rest),
        "help" | "-h" | "--help" => {
            crate::args::print_faf_sim_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown faf-sim command '{}'", other);
            crate::args::print_faf_sim_help();
            std::process::exit(1);
        }
    }
}

fn run_native(rest: &[String]) -> Result<()> {
    let release = rest.iter().any(|s| s == "--release");

    let mut cmd = cargo::command();
    cmd.args(["run", "--bin", "faf-sim", "--", "run"]);
    if release {
        cmd.arg("--release");
    }

    println!("Running native FAF Eco Sim ({})...", profile_str(release));
    let status = cmd.status().context("failed to run faf-sim")?;
    if !status.success() {
        bail!("faf-sim exited with status: {status}");
    }
    Ok(())
}

fn run_web(rest: &[String]) -> Result<()> {
    let mut command: Option<WebCommand> = None;
    let mut release = false;
    let mut port: Option<u16> = None;

    let mut iter = rest.iter().cloned();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "help" | "-h" | "--help" => {
                crate::args::print_faf_sim_help();
                std::process::exit(0);
            }
            "build" => {
                if command.is_some() {
                    bail!("multiple web commands given");
                }
                command = Some(WebCommand::Build);
            }
            "serve" => {
                if command.is_some() {
                    bail!("multiple web commands given");
                }
                command = Some(WebCommand::Serve);
            }
            "--release" => release = true,
            "--port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--port requires a value"))?;
                port = Some(
                    value
                        .parse()
                        .with_context(|| format!("invalid port number: {value}"))?,
                );
            }
            other if other.starts_with("--port=") => {
                let value = &other["--port=".len()..];
                port = Some(
                    value
                        .parse()
                        .with_context(|| format!("invalid port number: {value}"))?,
                );
            }
            other => {
                bail!("unknown web option '{}'", other);
            }
        }
    }

    web::run(
        &command.unwrap_or(WebCommand::Serve),
        release,
        port.unwrap_or(8080),
    )
}

fn profile_str(release: bool) -> &'static str {
    if release {
        "release"
    } else {
        "debug"
    }
}
