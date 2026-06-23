//! Command-line argument definitions for `faf-sim-cli`.
//!
//! This module composes the clap CLI using subcommands and typed enums so that
//! argument parsing is validated at parse time instead of deferring raw strings
//! to the dispatch logic in `main.rs`.
//!
//! Command structure:
//!
//! ```text
//! faf-sim <command> <faction> <unit> [options]
//! ```
//!
//! Faction is a subcommand; each faction exposes only its own valid units as a
//! `ValueEnum`, so clap constrains `<UNIT>` to faction-legal values.

use clap::{Parser, Subcommand, ValueEnum};

use crate::target::{AeonUnit, CybranUnit, SeraphimUnit, UefUnit};

/// Top-level CLI parser.
#[derive(Parser)]
#[command(name = "faf-sim")]
#[command(about = "Research CLI for FAF build-order simulation and optimization")]
#[command(after_help = crate::target::ResearchTarget::help_text())]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Available subcommands.
#[derive(Subcommand)]
pub enum Command {
    /// Show the dependency graph for a target unit.
    Deps(DepsArgs),
    /// Simulate a build order and print timing/resource trace.
    Simulate(SimulateArgs),
    /// Generate a build order for a target unit.
    Plan(PlanArgs),
}

/// Arguments for the `deps` subcommand.
#[derive(Parser)]
pub struct DepsArgs {
    /// Faction and unit to inspect.
    #[command(subcommand)]
    pub target: FactionTarget,
    /// Stop expanding prerequisites at these unit ids (default: commanders).
    #[arg(long, value_delimiter = ',', global = true)]
    pub stop_at: Vec<String>,
}

/// Arguments for the `simulate` subcommand.
#[derive(Parser)]
pub struct SimulateArgs {
    /// Planner strategy.
    #[arg(short = 's', long, default_value = "greedy", global = true)]
    pub strategy: StrategyArg,
    /// Faction and unit to simulate.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Arguments for the `plan` subcommand.
#[derive(Parser)]
pub struct PlanArgs {
    /// Planner strategy.
    #[arg(short = 's', long, default_value = "greedy", global = true)]
    pub strategy: StrategyArg,
    /// Faction and unit to plan for.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Faction subcommand. Each variant carries a faction-specific unit enum so
/// that clap can list only the units valid for that faction.
#[derive(Subcommand)]
pub enum FactionTarget {
    /// United Earth Federation.
    Uef(UefTargetArgs),
    /// Cybran Nation.
    Cybran(CybranTargetArgs),
    /// Aeon Illuminate.
    Aeon(AeonTargetArgs),
    /// Seraphim.
    Seraphim(SeraphimTargetArgs),
}

#[derive(Parser)]
pub struct UefTargetArgs {
    /// Unit to target.
    pub unit: UefUnit,
}

#[derive(Parser)]
pub struct CybranTargetArgs {
    /// Unit to target.
    pub unit: CybranUnit,
}

#[derive(Parser)]
pub struct AeonTargetArgs {
    /// Unit to target.
    pub unit: AeonUnit,
}

#[derive(Parser)]
pub struct SeraphimTargetArgs {
    /// Unit to target.
    pub unit: SeraphimUnit,
}

/// Selectable planner strategy.
///
/// This is a local clap-facing enum so that the library `Strategy` type does not
/// need to depend on clap.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum StrategyArg {
    /// Greedy state-machine policy.
    Greedy,
    /// Beam-search planner.
    Beam,
}

impl From<StrategyArg> for faf_sim::Strategy {
    fn from(arg: StrategyArg) -> Self {
        match arg {
            StrategyArg::Greedy => faf_sim::Strategy::Greedy,
            StrategyArg::Beam => faf_sim::Strategy::Beam,
        }
    }
}
