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
    Search {
        str: String,
    },
    /// Run a build-queue simulation and emit events as NDJSON.
    Build {
        plan_file: PathBuf,

        /// Playback speed in ticks per wall-clock second.
        ///
        /// The engine processes one fixed tick per `app.update()`.  A value of
        /// `1` runs one tick per real second; `10` runs ten ticks per real
        /// second.  Use `0` (default) to run as fast as the CPU allows.
        #[arg(long, default_value_t = 0.0)]
        speed: f64,
    },
}
