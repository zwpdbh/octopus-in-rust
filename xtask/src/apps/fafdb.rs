use anyhow::{Context, Result};

use crate::cargo;

/// Run a faf-db-specific command.
pub fn run(command: &str, _rest: &[String]) -> Result<()> {
    match command {
        "backend" => run_backend(),
        "frontend" => run_frontend(),
        "help" | "-h" | "--help" => {
            crate::args::print_faf_db_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown faf-db command '{}'", other);
            crate::args::print_faf_db_help();
            std::process::exit(1);
        }
    }
}

fn run_backend() -> Result<()> {
    let mut cmd = cargo::command();
    cmd.args(["run", "--package", "faf-db-server"]);

    println!("Starting FAF DB backend...");
    let status = cmd.status().context("failed to run faf-db-server")?;
    if !status.success() {
        anyhow::bail!("faf-db-server exited with status: {status}");
    }
    Ok(())
}

fn run_frontend() -> Result<()> {
    let mut cmd = std::process::Command::new("dx");
    cmd.args(["serve", "--platform", "web", "--port", "8080"]);
    cmd.current_dir("apps/faf-db-web");

    println!("Starting FAF DB frontend...");
    let status = cmd.status().context("failed to run dx serve")?;
    if !status.success() {
        anyhow::bail!("dx serve exited with status: {status}");
    }
    Ok(())
}
