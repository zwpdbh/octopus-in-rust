//! Command-line schema for `faf-sim-cli`.
//!
//! This module contains every `clap` argument, subcommand, and value enum used
//! by the binary. The actual command logic lives in sibling modules.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use faf_build_scheduler::AlgorithmKind;

#[derive(Parser, Debug)]
#[command(name = "faf-sim", about = "Headless FAF build-queue simulator")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a build-queue simulation and emit events as NDJSON.
    Build {
        #[command(subcommand)]
        mode: BuildMode,
    },
    /// Predict the completion time of a build plan using the analytical solver.
    Predict {
        #[command(subcommand)]
        mode: PredictMode,
    },
    /// Compute a build order that reaches an eco or unit target.
    Schedule {
        #[command(subcommand)]
        mode: ScheduleMode,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleMode {
    /// Find the fastest way to reach an eco target.
    Eco(ScheduleEcoArgs),
    /// Find the fastest way to build a target unit.
    Unit(ScheduleUnitArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ScheduleEcoArgs {
    /// Scheduling algorithm to use.
    #[arg(long, value_enum, default_value_t = AlgorithmKind::Greedy)]
    pub algorithm: AlgorithmKind,

    /// JSON file describing the scheduling request.
    ///
    /// The file may contain `initial_eco`, optional `initial_inventory`, and
    /// target production thresholds (`target_mass_production`,
    /// `target_energy_production`). If omitted, the request starts with a single
    /// ACU and the default initial economy.
    pub input: Option<PathBuf>,

    /// Target mass income per second. Overrides the value in `--input`.
    /// Defaults to 500 if no target is specified.
    #[arg(long)]
    pub target_mass_production: Option<f64>,

    /// Maximum number of mass extractors (including capped variants) allowed
    /// in the plan. Overrides the value in `--input`. Defaults to 10.
    #[arg(long)]
    pub max_mex: Option<u32>,

    /// Output path for the generated BuildQueue JSON.
    /// If omitted, the JSON is printed to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct ScheduleUnitArgs {
    /// Scheduling algorithm to use.
    #[arg(long, value_enum, default_value_t = AlgorithmKind::Greedy)]
    pub algorithm: AlgorithmKind,

    /// JSON file describing the scheduling request.
    ///
    /// The file may contain `initial_eco`, optional `initial_inventory`, and
    /// a `target` UnitKind string such as `Engineer(T1)`. If omitted, the
    /// request starts with a single ACU and the default initial economy, and
    /// `--target` must be provided.
    pub input: Option<PathBuf>,

    /// Target unit kind, such as `Engineer(T1)`. Overrides the value in `--input`.
    #[arg(long)]
    pub target: Option<String>,

    /// Output path for the generated BuildQueue JSON.
    /// If omitted, the JSON is printed to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum PredictMode {
    /// Predict using the analytical solver.
    Solver(PredictSolverArgs),
}

#[derive(Args, Debug, Clone)]
pub struct PredictSolverArgs {
    #[command(flatten)]
    pub shared: PredictShared,
    /// Safety cap on how many seconds the solver may run.
    #[arg(short, long, default_value = "6000")]
    pub max_time_seconds: f64,
}

#[derive(Args, Debug, Clone)]
pub struct PredictShared {
    /// JSON file with the initial economy snapshot.
    /// If omitted, the snapshot is derived from the plan's `initial_eco` field.
    #[arg(short, long)]
    pub eco: Option<PathBuf>,
    /// JSON file with the build plan (`BuildQueue`).
    #[arg(short, long)]
    pub plan: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum BuildMode {
    /// Run the simulation in active (manual-advance) mode.
    Active {
        /// JSON file describing the build queue.
        queue: PathBuf,
        #[command(flatten)]
        shared: BuildShared,
    },
    /// Run the simulation in passive (auto-play) mode.
    Passive {
        /// JSON file describing the build queue.
        queue: PathBuf,
        #[command(flatten)]
        shared: BuildShared,
        /// Real-world delay between simulation steps in milliseconds.
        #[arg(long, default_value = "50")]
        tick_interval_ms: u64,
    },
}

#[derive(Args, Debug, Clone, Copy)]
pub struct BuildShared {
    /// Simulation step size in seconds. Must be an integer >= 1.
    #[arg(short, long, default_value = "1")]
    pub dt_seconds: u32,
    /// Maximum simulation time in seconds. When omitted the simulation
    /// runs until the build queue is empty.
    #[arg(short, long)]
    pub max_time_seconds: Option<u32>,
    /// Output format for Ticked events.
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Raw)]
    pub format: OutputFormat,
    /// How many seconds the simulation keeps ticking after the build queue is
    /// empty. A value of `0` stops immediately at completion and prints only
    /// the final result, suppressing intermediate events.
    #[arg(long, default_value_t = 30.0)]
    pub tail_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Emit raw simulation events (the primitive data source).
    Raw,
    /// Emit Ticked events grouped into rates/storage/totals/derived.
    Grouped,
}
