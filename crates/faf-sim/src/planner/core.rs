//! Planner abstraction for build-order generation.
//!
//! A [`Planner`] turns an initial [`SimulationState`] into a [`PlanResult`]
//! (timeline + completion time). It dispatches to the learned policy strategy.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::economy::EconomyState;
use crate::planner::policy::direction_planner;
use crate::planner::policy::value_net::ValueNet;
use crate::sim::{BuildEvent, SimulationState};
use crate::units::{TechLevel, UnitCost, Units};

/// Abstract target for planning and training.
///
/// The planner and trainer no longer need to know the specific unit being built.
/// A goal is fully described by the tech level required to build it and its
/// resource cost. The concrete unit specified on the CLI is resolved into this
/// abstraction before training or simulation begins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goal {
    /// Highest tech level required to build the target.
    pub tech_level: TechLevel,
    /// Mass cost of the target.
    pub mass_cost: f64,
    /// Energy cost of the target.
    pub energy_cost: f64,
    /// Build-time cost of the target (in seconds of build power).
    pub build_time: f64,
}

impl Goal {
    /// Convenience accessor for the combined cost.
    pub fn cost(&self) -> UnitCost {
        UnitCost {
            mass: self.mass_cost,
            energy: self.energy_cost,
            build_time: self.build_time,
        }
    }
}

/// Result of running a planner from a single state.
///
/// The planner searches a trajectory from the current state to the goal. Most
/// consumers should treat this as a **one-step lookahead**: execute
/// [`PlanResult::first_action`], advance the simulator, and call the planner
/// again. The remaining fields (`events`, `completion_time`, `final_economy`)
/// are the projected full trajectory used internally for value estimation;
/// they are not a fixed schedule and are typically ignored by the reactive
/// execution loop.
#[derive(Debug, Clone)]
pub struct PlanResult {
    /// Projected completion events in chronological order.
    ///
    /// This is what the policy expects to happen if the current best action
    /// sequence is followed to the goal. In a reactive loop the simulator
    /// produces its own authoritative events; use those instead.
    pub events: Vec<BuildEvent>,
    /// Projected in-game seconds when the goal unit would finish.
    pub completion_time: f64,
    /// Projected economy state at the end of the plan.
    pub final_economy: EconomyState,
    /// Immediate next action chosen by the planner.
    ///
    /// In the closed-loop actor design this is the only field the executor
    /// commits to. It is converted into a [`crate::actors::message::SimulationMsg`]
    /// and sent to the simulator; the rest of the plan is recomputed next tick.
    pub first_action: Option<crate::planner::SimAction>,
    /// State after executing [`first_action`].
    ///
    /// This lets callers plan several steps ahead without waiting for the
    /// simulator to report back after every command.
    pub final_state: SimulationState,
}

/// Planner error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannerError {
    /// Requested strategy is not implemented.
    UnsupportedStrategy(String),
    /// The simulation did not reach the goal unit.
    SimulationFailed,
    /// The search ran out of states before reaching the goal.
    SearchExhausted,
    /// A generic planner error.
    Other(String),
}

impl fmt::Display for PlannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlannerError::UnsupportedStrategy(name) => {
                write!(f, "strategy '{}' is not implemented", name)
            }
            PlannerError::SimulationFailed => {
                write!(f, "simulation failed to reach the goal unit")
            }
            PlannerError::SearchExhausted => {
                write!(f, "search exhausted without reaching the goal")
            }
            PlannerError::Other(msg) => {
                write!(f, "{}", msg)
            }
        }
    }
}

impl Error for PlannerError {}

/// Architecture of the learned policy network.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ValueNetKind {
    /// Hierarchical policy bundle (macro + build-power + engineer-squad).
    #[default]
    Mlp,
    /// Graph neural network that reasons over the plan graph structure.
    Gnn,
}

impl fmt::Display for ValueNetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueNetKind::Mlp => write!(f, "mlp"),
            ValueNetKind::Gnn => write!(f, "gnn"),
        }
    }
}

impl FromStr for ValueNetKind {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "mlp" => Ok(ValueNetKind::Mlp),
            "gnn" => Ok(ValueNetKind::Gnn),
            _ => Err(PlannerError::UnsupportedStrategy(format!(
                "unknown value net kind '{}'",
                s
            ))),
        }
    }
}

/// Selectable planning algorithm.
///
/// The default `Policy` strategy runs the trained network once per decision,
/// masks illegal directions, and picks the highest-probability legal direction
/// (or samples when stochastic). This is the same path used during training
/// episodes and is fast enough for the reactive simulation loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Direct one-step policy lookup guided by a learned network.
    Policy {
        /// Kind of learned policy network to use.
        value_net: ValueNetKind,
        /// If true, always pick the highest-scoring legal direction; otherwise
        /// sample from the masked softmax.
        deterministic: bool,
    },
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        "policy"
    }

    /// Return a copy of this strategy with deterministic selection enabled.
    pub fn with_deterministic(self) -> Self {
        let Strategy::Policy { value_net, .. } = self;
        Strategy::Policy {
            value_net,
            deterministic: true,
        }
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();

        if lower == "policy" {
            return Ok(Strategy::Policy {
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            });
        }

        if let Some(rest) = lower.strip_prefix("policy") {
            let parts: Vec<&str> = rest.split(':').filter(|p| !p.is_empty()).collect();
            let mut value_net = ValueNetKind::Mlp;
            let mut deterministic = false;
            for part in parts {
                if part == "greedy" || part == "deterministic" {
                    deterministic = true;
                } else {
                    value_net = ValueNetKind::from_str(part)?;
                }
            }
            return Ok(Strategy::Policy {
                value_net,
                deterministic,
            });
        }

        Err(PlannerError::UnsupportedStrategy(s.to_string()))
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Strategy::Policy {
            value_net,
            deterministic,
        } = self;
        if *deterministic {
            write!(f, "policy:{}:greedy", value_net)
        } else {
            write!(f, "policy:{}", value_net)
        }
    }
}

/// Configuration shared by all planning strategies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannerConfig {
    /// Fixed simulation timestep in seconds.
    pub dt: f64,
    /// Maximum number of search layers to explore.
    pub max_depth: usize,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Probability threshold above which the rush head chooses to start the goal.
    pub rush_threshold: f64,
}

impl Default for PlannerConfig {
    /// Defaults tuned for the reactive policy loop with a 1-second decision timestep.
    fn default() -> Self {
        Self {
            dt: 1.0,
            max_depth: 400,
            max_mex_count: 12,
            rush_threshold: 0.5,
        }
    }
}

/// A planner that dispatches to the learned policy strategy.
///
/// The concrete policy network is hidden behind a [`ValueNet`] trait object;
/// the planner only knows the strategy and planner configuration. Training
/// details such as the Burn backend or the hierarchical network architecture
/// are not part of the public surface.
///
/// A planner must be created with a value net. If you do not have a trained
/// model, pass a freshly-initialized net (e.g. `MlpValueNet::new`); the
/// planner itself will not silently fall back to a random policy.
#[derive(Debug)]
pub struct Planner {
    /// Selected planning strategy.
    pub strategy: Strategy,
    /// Shared search configuration.
    pub config: PlannerConfig,
    /// Value net used for inference.
    value_net: Box<dyn ValueNet>,
}

impl Clone for Planner {
    fn clone(&self) -> Self {
        Self {
            strategy: self.strategy,
            config: self.config,
            value_net: self.value_net.clone_box(),
        }
    }
}

impl Planner {
    /// Create a planner with the default configuration.
    pub fn new(strategy: Strategy, value_net: Box<dyn ValueNet>) -> Self {
        Self::with_config(strategy, PlannerConfig::default(), value_net)
    }

    /// Create a planner tuned for the reactive actor loop.
    pub fn reactive(strategy: Strategy, value_net: Box<dyn ValueNet>) -> Self {
        Self::with_config(strategy, PlannerConfig::default(), value_net)
    }

    /// Create a planner with an explicit configuration.
    pub fn with_config(
        strategy: Strategy,
        config: PlannerConfig,
        value_net: Box<dyn ValueNet>,
    ) -> Self {
        Self {
            strategy,
            config,
            value_net,
        }
    }

    /// Run the planner from `initial_state` toward `goal`.
    ///
    /// This is the public entry point for planning. It evaluates the learned
    /// policy once, masks illegal directions, and returns a [`PlanResult`] that
    /// contains both a projected full trajectory and the immediate next action.
    ///
    /// The result is designed for **closed-loop, reactive** use: the caller should
    /// execute only [`PlanResult::first_action`], advance the simulator, and call
    /// `plan` again on the new state. The projected `events`, `completion_time`,
    /// and `final_economy` are the policy's best estimate of what would happen if
    /// the current policy were followed to the goal, but they are not a fixed
    /// schedule. Replanning every tick prevents discrete timing drift (stalls,
    /// builder rounding, completed projects) from compounding.
    pub fn plan(
        &mut self,
        units: &Units,
        initial_state: SimulationState,
        goal: &Goal,
    ) -> Result<PlanResult, PlannerError> {
        let Strategy::Policy {
            value_net,
            deterministic,
        } = self.strategy;
        direction_planner::plan(
            units,
            initial_state,
            goal,
            1,
            value_net,
            deterministic,
            Some(self.value_net.as_ref()),
            &self.config,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_parses_policy() {
        assert_eq!(
            Strategy::from_str("policy").unwrap(),
            Strategy::Policy {
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("policy:greedy").unwrap(),
            Strategy::Policy {
                value_net: ValueNetKind::Mlp,
                deterministic: true,
            }
        );
        assert_eq!(
            Strategy::from_str("Policy:gnn").unwrap(),
            Strategy::Policy {
                value_net: ValueNetKind::Gnn,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("policy:gnn:greedy").unwrap(),
            Strategy::Policy {
                value_net: ValueNetKind::Gnn,
                deterministic: true,
            }
        );
    }

    #[test]
    fn strategy_display_includes_value_net() {
        assert_eq!(
            Strategy::Policy {
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            }
            .to_string(),
            "policy:mlp"
        );
        assert_eq!(
            Strategy::Policy {
                value_net: ValueNetKind::Mlp,
                deterministic: true,
            }
            .to_string(),
            "policy:mlp:greedy"
        );
    }

    #[test]
    fn unknown_strategy_errors() {
        assert!(matches!(
            Strategy::from_str("greedy"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
        assert!(matches!(
            Strategy::from_str("beam:20"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
        assert!(matches!(
            // The old MCTS strategy is no longer supported.
            Strategy::from_str("mcts:abc"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
        assert!(matches!(
            Strategy::from_str("policy:100"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
    }
}
