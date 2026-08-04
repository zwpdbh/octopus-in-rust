//! CLI for the FAF build-queue simulator and build-order scheduler.
//!
//! This binary dispatches to focused modules in `src/*.rs`. Most commands are
//! thin wrappers over the `faf-sim` and `faf-sim-service` crates.

mod build;
mod command_line;
mod util;

use clap::Parser;
use command_line::Cli;

use crate::command_line::Command::Build;
use anyhow::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Build { queue } => {
            #[allow(unused)]
            let json = std::fs::read_to_string(&queue).unwrap_or_else(|e| {
                eprintln!("Failed to read: {}, error: {}", queue.display(), e);
                std::process::exit(1);
            });

            let _ = build::run()?;
        }
    }
    Ok(())
}
