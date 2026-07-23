//! CLI for the FAF build-queue simulator and build-order scheduler.
//!
//! This binary dispatches to focused modules in `src/*.rs`. Most commands are
//! thin wrappers over the `faf-sim` and `faf-sim-service` crates.

mod build;
mod command_line;
mod predict;
mod schedule;
mod util;

use clap::Parser;
use command_line::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { mode } => build::run(mode),
        Command::Predict { mode } => predict::run(mode),
        Command::Schedule { mode } => schedule::run(mode),
    }
}
