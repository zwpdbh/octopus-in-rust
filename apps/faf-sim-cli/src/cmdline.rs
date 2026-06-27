//! Command-line argument definitions for `faf-sim-cli`.
//!
//! This module composes the clap CLI using subcommands and typed enums so that
//! argument parsing is validated at parse time instead of deferring raw strings
//! to the dispatch logic in `main.rs`.
//!
//! Command structure:
//!
//! ```text
//! faf-sim <command> [strategy-options] <faction> <unit>
//! ```
//!
//! For `plan` and `simulate`, the target faction/unit is a subcommand so clap
//! can constrain `<UNIT>` to faction-legal values. The planner strategy is a
//! single typed argument that carries any strategy-specific configuration
//! (e.g. `beam:20`) inside its value.

use clap::{Parser, Subcommand};

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
    /// Generate a rule-based build plan for a target unit.
    ///
    /// The output is an SVG image of the dependency graph showing the units
    /// that must be built (or upgraded) to reach the goal. No timing or
    /// resource simulation is performed; this is purely symbolic dependency
    /// planning.
    Plan(PlanArgs),
    /// Simulate a build order and print timing/resource trace.
    ///
    /// Uses the symbolic plan graph together with a planner strategy to explore
    /// an estimated completion timeline.
    Simulate(SimulateArgs),
}

/// Arguments for the `plan` subcommand.
#[derive(Parser)]
pub struct PlanArgs {
    /// Faction and unit to target.
    #[command(subcommand)]
    pub target: FactionTarget,
    /// Write the SVG plan to this file instead of a temporary file.
    #[arg(short = 'o', long)]
    pub output: Option<std::path::PathBuf>,
}

/// Arguments for the `simulate` subcommand.
#[derive(Parser)]
pub struct SimulateArgs {
    /// Planner strategy (`greedy`, `beam`, or `beam:<width>`).
    #[arg(short = 's', long, default_value = "greedy")]
    pub strategy: faf_sim::Strategy,
    /// Faction and unit to target.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Faction subcommand. Each variant carries a faction-specific unit enum so
/// that clap can list only the units valid for that faction.
#[derive(Debug, Clone, Subcommand)]
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

#[derive(Debug, Clone, Parser)]
pub struct UefTargetArgs {
    /// Unit to target.
    pub unit: UefUnit,
}

#[derive(Debug, Clone, Parser)]
pub struct CybranTargetArgs {
    /// Unit to target.
    pub unit: CybranUnit,
}

#[derive(Debug, Clone, Parser)]
pub struct AeonTargetArgs {
    /// Unit to target.
    pub unit: AeonUnit,
}

#[derive(Debug, Clone, Parser)]
pub struct SeraphimTargetArgs {
    /// Unit to target.
    pub unit: SeraphimUnit,
}
