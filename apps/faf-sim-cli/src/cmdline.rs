//! Command-line argument definitions for `faf-sim-cli`.
//!
//! This module composes the clap CLI using subcommands and typed enums so that
//! argument parsing is validated at parse time instead of deferring raw strings
//! to the dispatch logic in `main.rs`.
//!
//! Command structure:
//!
//! ```text
//! faf-sim plan
//! faf-sim train eco
//! faf-sim train rush <faction> <unit>
//! faf-sim simulate eco
//! faf-sim simulate rush <faction> <unit>
//! faf-sim draw-net <faction> <unit>
//! ```

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

/// Available top-level subcommands.
#[derive(Subcommand)]
pub enum Command {
    /// Generate an SVG of the universal build/upgrade plan graph.
    ///
    /// The output is an SVG image of the ACU-rooted dependency graph showing all
    /// units and their build/upgrade relationships. No timing or resource
    /// simulation is performed; this is purely symbolic dependency planning.
    Plan(PlanArgs),
    /// Train a learned planner network.
    ///
    /// `train eco` trains a network to grow mass income.  `train rush` trains
    /// the goal-rush network for a target unit.
    Train {
        #[command(subcommand)]
        subcommand: TrainSubcommand,
    },
    /// Simulate a planner on an initial ACU state.
    ///
    /// `simulate eco` runs the eco planner for a fixed number of steps.
    /// `simulate rush` runs a full build-order simulation toward a target unit.
    Simulate {
        #[command(subcommand)]
        subcommand: SimulateSubcommand,
    },
    /// Draw the value-network architecture for a target unit.
    ///
    /// Emits a Graphviz DOT description of the hierarchical policy network and
    /// renders it to SVG if Graphviz is installed.
    DrawNet(DrawNetArgs),
}

/// Subcommands for `faf-sim train`.
#[derive(Subcommand)]
pub enum TrainSubcommand {
    /// Train a standalone economy-expansion network.
    ///
    /// The network learns to grow mass income as fast as possible toward a fixed
    /// target. No final unit/goal is required.
    Eco(TrainEcoArgs),
    /// Train an MLP value network for a target unit.
    ///
    /// Runs policy-gradient rollouts and saves the trained model so that
    /// `simulate rush` can use it instead of a randomly initialized network.
    Rush(TrainRushArgs),
}

/// Subcommands for `faf-sim simulate`.
#[derive(Subcommand)]
pub enum SimulateSubcommand {
    /// Run the standalone eco planner for a number of steps.
    ///
    /// Grows mass income as fast as possible using the heuristic or a loaded
    /// policy network, stopping when the target income is reached or the step
    /// budget is exhausted.
    Eco(SimulateEcoArgs),
    /// Run a full build-order simulation toward a target unit.
    ///
    /// Uses a trained rush policy to plan and execute a complete build order,
    /// producing a timeline and an SVG diagram.
    Rush(SimulateRushArgs),
}

/// Arguments for the `plan` subcommand.
#[derive(Parser)]
pub struct PlanArgs {
    /// Write the SVG plan to this file instead of a temporary file.
    #[arg(short = 'o', long)]
    pub output: Option<std::path::PathBuf>,
}

/// Arguments for `faf-sim train eco`.
#[derive(Parser)]
#[command(
    after_help = "Examples:\n  cargo run --release --bin faf-sim -- train eco -e 2000 -m 10000"
)]
pub struct TrainEcoArgs {
    /// Number of training episodes. Must be specified with `-e`.
    #[arg(short = 'e', long)]
    pub episodes: usize,
    /// Maximum simulator steps per episode.
    #[arg(short = 'm', long, default_value = "500")]
    pub max_steps: usize,
    /// Fixed simulator timestep for rollouts, in seconds.
    #[arg(long, default_value = "1.0")]
    pub dt: f64,
    /// Resume training from an existing eco model, if one exists.
    #[arg(short = 'r', long, default_value = "false")]
    pub resume: bool,
    /// Delete any existing eco model checkpoint before training.
    #[arg(long, default_value = "false")]
    pub fresh: bool,
    /// Target mass income per second. Training episodes end when this income is
    /// reached.
    #[arg(long, default_value = "1000.0")]
    pub target_mass_income: f64,
    /// Learning rate for Adam.
    #[arg(long, default_value = "1e-3")]
    pub learning_rate: f64,
    /// Clip gradients by global L2 norm to this value.
    #[arg(long)]
    pub grad_clip: Option<f32>,
    /// Maximum number of mass extractors (including capped upgrades) that may
    /// be active at the same time.
    #[arg(long, default_value = "12")]
    pub max_mex_count: usize,
    /// Coefficient for the mass-income delta reward.
    #[arg(long, default_value = "0.1")]
    pub reward_mass_income_coef: f32,
    /// Penalty applied each step when energy storage is empty.
    #[arg(long, default_value = "20.0")]
    pub energy_stall_penalty: f32,
    /// Initial epsilon for exploration.
    #[arg(long, default_value = "0.3")]
    pub epsilon_start: f32,
    /// Final epsilon after decay.
    #[arg(long, default_value = "0.01")]
    pub epsilon_end: f32,
    /// Number of episodes over which epsilon decays.
    #[arg(long, default_value = "1000")]
    pub epsilon_decay_episodes: usize,
}

/// Arguments for `faf-sim train rush`.
#[derive(Parser)]
#[command(
    after_help = "Examples:\n  cargo run --release --bin faf-sim -- train rush -e 2000 -m 10000 -r uef fatboy"
)]
pub struct TrainRushArgs {
    /// Number of training episodes. Must be specified with `-e`. Use `0` to run
    /// until the target time is reached or the process is interrupted.
    #[arg(short = 'e', long)]
    pub episodes: usize,
    /// Maximum simulator steps per episode.
    #[arg(short = 'm', long, default_value = "500")]
    pub max_steps: usize,
    /// Fixed simulator timestep for rollouts, in seconds. Smaller values run the
    /// simulator more finely but require more steps to cover the same game time.
    #[arg(long, default_value = "1.0")]
    pub dt: f64,
    /// Resume training from an existing model for this target, if one exists.
    #[arg(short = 'r', long, default_value = "false")]
    pub resume: bool,
    /// Delete any existing model checkpoint for this target before training.
    /// Useful for starting a fresh run without manually removing `data/models/`.
    #[arg(long, default_value = "false")]
    pub fresh: bool,
    /// Stop training early once the best completion time is at most this
    /// duration. Accepts plain seconds or a suffix (`30m`, `1h`, `1200s`).
    #[arg(short = 't', long, value_parser = parse_duration)]
    pub target_time: Option<f64>,
    /// Suppress per-episode and progress output.
    #[arg(long, default_value = "false")]
    pub quiet: bool,
    /// Print plain-text progress to stderr instead of opening the terminal dashboard.
    #[arg(long, default_value = "false")]
    pub text: bool,
    /// Clip gradients by global L2 norm to this value. `1.0` is a good default
    /// for preventing REINFORCE divergence; omit to disable clipping.
    #[arg(long)]
    pub grad_clip: Option<f32>,
    /// Maximum number of mass extractors (including capped upgrades) that may
    /// be active at the same time. New mex builds are blocked once this cap is
    /// reached; upgrades do not count toward the cap.
    #[arg(long, default_value = "12")]
    pub max_mex_count: usize,
    /// Coefficient for the build-power delta reward. Set to 0.0 to disable.
    #[arg(long, default_value = "0.05")]
    pub reward_bp_coef: f32,
    /// Coefficient for the energy-income delta reward. Default is 0.0 so the
    /// agent learns power management from the energy stall penalty instead of
    /// a direct income bonus that can encourage overbuilding power generators.
    #[arg(long, default_value = "0.0")]
    pub reward_energy_income_coef: f32,
    /// Horizon in seconds for the phantom-goal eco rollout.
    #[arg(long, default_value = "60.0")]
    pub eco_rollout_horizon_secs: f32,
    /// Maximum seconds to simulate when evaluating a real goal rush.
    #[arg(long, default_value = "300.0")]
    pub rush_rollout_cap_secs: f32,
    /// Fraction of total build power assigned to the phantom/rush goal project.
    #[arg(long, default_value = "0.8")]
    pub rollout_bp_fraction: f32,
    /// Coefficient scaling the delta in mass spent during the eco rollout.
    #[arg(long, default_value = "0.01")]
    pub mass_reward_coef: f32,
    /// Base reward for finishing the real goal within the rush cap.
    #[arg(long, default_value = "100.0")]
    pub goal_finish_base_reward: f32,
    /// Penalty for picking Goal when the goal cannot finish within the rush cap.
    #[arg(long, default_value = "-10.0")]
    pub goal_too_early_penalty: f32,
    /// Initial epsilon for Goal-only exploration.
    #[arg(long, default_value = "0.3")]
    pub epsilon_start: f32,
    /// Rush probability threshold above which Goal is chosen (outside exploration).
    #[arg(long, default_value = "0.5")]
    pub rush_threshold: f32,
    /// Faction and unit to target.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Arguments for `faf-sim simulate eco`.
#[derive(Parser)]
#[command(
    after_help = "Examples:\n  cargo run --release --bin faf-sim -- simulate eco -n 200 -t 500"
)]
pub struct SimulateEcoArgs {
    /// Maximum number of planner steps to run.
    #[arg(short = 'n', long, default_value = "200")]
    pub steps: usize,
    /// Target mass income per second. The planner stops once this income is
    /// reached.
    #[arg(short = 't', long, default_value = "1000.0")]
    pub target_mass_income: f64,
    /// Fixed simulator timestep for rollouts, in seconds.
    #[arg(long, default_value = "1.0")]
    pub dt: f64,
    /// Maximum number of mass extractors (including capped upgrades) that may
    /// be active at the same time.
    #[arg(long, default_value = "12")]
    pub max_mex_count: usize,
    /// Load a trained eco policy network instead of using the heuristic.
    #[arg(short = 'm', long)]
    pub model: Option<std::path::PathBuf>,
}

/// Arguments for `faf-sim simulate rush`.
///
/// Runs a full build-order simulation toward the target using a trained model.
#[derive(Parser)]
#[command(
    after_help = "Examples:\n  cargo run --release --bin faf-sim -- simulate rush -s policy:mlp:greedy uef fatboy"
)]
pub struct SimulateRushArgs {
    /// Planner strategy (`policy`, `policy:<mlp|gnn>`, or append `:greedy` for
    /// deterministic argmax selection).
    #[arg(short = 's', long, default_value = "policy:mlp:greedy")]
    pub strategy: faf_sim::Strategy,
    /// Maximum number of mass extractors (including capped upgrades) that may
    /// be active at the same time. New mex builds are blocked once this cap is
    /// reached; upgrades do not count toward the cap.
    #[arg(long, default_value = "12")]
    pub max_mex_count: usize,
    /// Write the SVG build-order diagram to this file instead of a temporary file.
    #[arg(short = 'o', long)]
    pub output: Option<std::path::PathBuf>,
    /// Faction and unit to target.
    #[command(subcommand)]
    pub target: FactionTarget,
}

/// Arguments for the `draw-net` subcommand.
#[derive(Parser)]
pub struct DrawNetArgs {
    /// Write the DOT source to this file instead of a temporary file.
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

/// Cybran-specific target arguments.
#[derive(Debug, Clone, Parser)]
pub struct CybranTargetArgs {
    /// Cybran experimental or tech unit to target.
    pub unit: CybranUnit,
}

/// UEF-specific target arguments.
#[derive(Debug, Clone, Parser)]
pub struct UefTargetArgs {
    /// UEF experimental or tech unit to target.
    pub unit: UefUnit,
}

/// Aeon-specific target arguments.
#[derive(Debug, Clone, Parser)]
pub struct AeonTargetArgs {
    /// Aeon experimental or tech unit to target.
    pub unit: AeonUnit,
}

/// Seraphim-specific target arguments.
#[derive(Debug, Clone, Parser)]
pub struct SeraphimTargetArgs {
    /// Seraphim experimental or tech unit to target.
    pub unit: SeraphimUnit,
}
