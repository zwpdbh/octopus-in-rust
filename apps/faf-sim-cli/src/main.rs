//! CLI for the FAF build-queue simulator and build-time predictor.
//!
//! This binary dispatches to focused modules in `src/*.rs`. Most commands are
//! thin wrappers over the `faf-sim`, `faf-sim-service`, and
//! `faf-build-prediction` crates.

mod build;
mod command_line;
mod dataset;
mod predict;
mod train;
mod util;

use clap::Parser;
use command_line::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { mode } => build::run(mode),
        Command::Dataset { mode } => dataset::run(mode),
        Command::Train(args) => train::run(args),
        Command::Predict { mode } => predict::run(mode),
    }
}
