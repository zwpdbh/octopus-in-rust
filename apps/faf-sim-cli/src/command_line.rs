//! Command-line schema for `faf-sim-cli`.
//!
//! This module contains every `clap` argument, subcommand, and value enum used
//! by the binary. The actual command logic lives in sibling modules.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "faf-sim", about = "Headless FAF build-queue simulator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a build-queue simulation and emit events as NDJSON.
    Build { queue: PathBuf },
}
