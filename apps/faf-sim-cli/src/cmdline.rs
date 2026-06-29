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
//! (e.g. `mcts:200`) inside its value.

use clap::{Parser, Subcommand};

use crate::target::{AeonUnit, CybranUnit, SeraphimUnit, UefUnit};

/// Parse a human-friendly duration into seconds.
///
/// Accepts a plain number (interpreted as seconds) or a value with an `s`,
/// `m`, or `h` suffix.
pub fn parse_duration(s: &str) -> Result<f64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("duration must not be empty".to_string());
    }
    if let Some(num) = s.strip_suffix('h') {
        num.parse::<f64>()
            .map(|v| v * 3600.0)
            .map_err(|e| format!("invalid hours: {}", e))
    } else if let Some(num) = s.strip_suffix('m') {
        num.parse::<f64>()
            .map(|v| v * 60.0)
            .map_err(|e| format!("invalid minutes: {}", e))
    } else if let Some(num) = s.strip_suffix('s') {
        num.parse::<f64>()
            .map_err(|e| format!("invalid seconds: {}", e))
    } else {
        s.parse::<f64>()
            .map_err(|e| format!("invalid seconds: {}", e))
    }
}

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
    /// Train an MLP value network for a target unit.
    ///
    /// Runs policy-gradient rollouts and saves the trained model so that
    /// `simulate` can use it instead of a randomly initialized network.
    Train(TrainArgs),
    /// Simulate a build order and print timing/resource trace.
    ///
    /// Uses the symbolic plan graph together with a planner strategy to explore
    /// an estimated completion timeline. If a trained model exists for the
    /// target, it is loaded automatically.
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

/// Arguments for the `train` subcommand.
#[derive(Parser)]
#[command(
    after_help = "Examples:\n  cargo run --release --bin faf-sim -- train -e 2000 -m 10000 -r --epsilon 0.3 --epsilon-final 0.01 uef fatboy"
)]
pub struct TrainArgs {
    /// Number of training episodes. Must be specified with `-e`. Use `0` to run
    /// until the target time is reached or the process is interrupted.
    #[arg(short = 'e', long)]
    pub episodes: usize,
    /// Maximum simulator steps per episode.
    #[arg(short = 'm', long, default_value = "500")]
    pub max_steps: usize,
    /// Resume training from an existing model for this target, if one exists.
    #[arg(short = 'r', long, default_value = "false")]
    pub resume: bool,
    /// Stop training early once the best completion time is at most this
    /// duration. Accepts plain seconds or a suffix (`30m`, `1h`, `1200s`).
    #[arg(short = 't', long, value_parser = parse_duration)]
    pub target_time: Option<f64>,
    /// Initial epsilon-greedy exploration probability.
    #[arg(long, default_value = "0.1")]
    pub epsilon: f32,
    /// Final epsilon value after decay. Only used with `--epsilon-decay-episodes`.
    #[arg(long, default_value = "0.01")]
    pub epsilon_final: f32,
    /// Number of episodes over which to linearly decay epsilon from `--epsilon`
    /// to `--epsilon-final`. Defaults to the value of `-e`; pass `0` to disable
    /// decay entirely.
    #[arg(long)]
    pub epsilon_decay_episodes: Option<usize>,
    /// Faction and unit to target.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Arguments for the `simulate` subcommand.
#[derive(Parser)]
pub struct SimulateArgs {
    /// Planner strategy (`mcts`, `mcts:<iterations>`, or `mcts:<iterations>:<mlp|gnn>`).
    #[arg(short = 's', long, default_value = "mcts:100:mlp")]
    pub strategy: faf_sim::Strategy,
    /// Write the SVG build-order diagram to this file instead of a temporary file.
    #[arg(short = 'o', long)]
    pub output: Option<std::path::PathBuf>,
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
