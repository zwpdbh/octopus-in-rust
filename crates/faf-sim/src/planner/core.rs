//! Planner abstraction for build-order generation.
//!
//! A [`Planner`] turns an initial [`SimulationState`] into a [`PlanResult`]
//! (timeline + completion time). It dispatches to the MCTS strategy.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::economy::EconomyState;
use crate::planner::mcts::macro_net::{num_plan_edges, PolicyBundle};
use crate::planner::mcts::search::{MctsConfig, MctsSearch};
use crate::planner::mcts::train::{TrainBackend, TrainDevice};
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

/// Result of running a planner to completion.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    /// Completion events in chronological order.
    pub events: Vec<BuildEvent>,
    /// In-game seconds when the goal unit finished.
    pub completion_time: f64,
    /// Economy state at the end of the plan.
    pub final_economy: EconomyState,
    /// First action of the best path. Useful for reactive planners that only
    /// commit to the immediate next step.
    pub first_action: Option<crate::planner::search::SimAction>,
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

/// Architecture of the learned value network used inside MCTS.
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
/// Currently only MCTS is supported, but the value network that guides it can be
/// chosen between MLP and GNN. The enum is kept so future strategies can be
/// added without changing the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Monte Carlo Tree Search guided by a learned value network.
    Mcts {
        /// Number of MCTS iterations to run per decision.
        iterations: usize,
        /// Kind of learned value network to use inside MCTS.
        value_net: ValueNetKind,
        /// If true, the policy always picks the highest-scoring legal plan-graph
        /// edge instead of sampling from the softmax. This makes simulation
        /// deterministic and reproducible.
        deterministic: bool,
    },
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        "mcts"
    }

    /// Return a copy of this strategy with deterministic selection enabled.
    pub fn with_deterministic(self) -> Self {
        match self {
            Strategy::Mcts {
                iterations,
                value_net,
                ..
            } => Strategy::Mcts {
                iterations,
                value_net,
                deterministic: true,
            },
        }
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        if lower == "mcts" {
            return Ok(Strategy::Mcts {
                iterations: 100,
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            });
        }

        let Some(rest) = lower.strip_prefix("mcts") else {
            return Err(PlannerError::UnsupportedStrategy(s.to_string()));
        };

        // Supported forms:
        //   mcts:<iter>
        //   mcts:<iter>:<net>
        //   mcts::<net>              (default iterations)
        //   mcts:<iter>:<net>:greedy (deterministic argmax)
        //   mcts:greedy              (deterministic, defaults)
        let parts: Vec<&str> = rest.split(':').filter(|p| !p.is_empty()).collect();

        let mut iterations = 100usize;
        let mut value_net = ValueNetKind::Mlp;
        let mut deterministic = false;

        for part in parts {
            if part == "greedy" || part == "deterministic" {
                deterministic = true;
            } else if let Ok(iters) = part.parse::<usize>() {
                iterations = iters;
            } else {
                value_net = ValueNetKind::from_str(part)?;
            }
        }

        Ok(Strategy::Mcts {
            iterations,
            value_net,
            deterministic,
        })
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strategy::Mcts {
                iterations,
                value_net,
                deterministic,
            } => {
                if *deterministic {
                    write!(f, "mcts:{}:{}:greedy", iterations, value_net)
                } else {
                    write!(f, "mcts:{}:{}", iterations, value_net)
                }
            }
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
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
    /// Maximum number of energy storage buildings to build.
    pub max_energy_storage_count: usize,
}

impl Default for PlannerConfig {
    /// Defaults tuned for MCTS with a 1-second decision timestep.
    fn default() -> Self {
        Self {
            dt: 1.0,
            max_depth: 400,
            max_mex_count: 12,
            max_pgen_count: 30,
            max_energy_storage_count: 80,
        }
    }
}

/// A planner that dispatches to the MCTS strategy.
#[derive(Debug, Clone)]
pub struct Planner {
    /// Selected planning strategy.
    pub strategy: Strategy,
    /// Shared search configuration.
    pub config: PlannerConfig,
    /// Optional trained hierarchical policy bundle. If present, MCTS uses it
    /// instead of a fresh random initialization.
    pub value_net: Option<PolicyBundle<TrainBackend>>,
    /// Lazily-created random model used when no trained value net is available.
    default_model: Option<PolicyBundle<TrainBackend>>,
    /// Previous-tick engineer shortfall feedback passed to the macro network.
    pub last_shortfall: [f32; 3],
}

impl Planner {
    /// Create a planner with the default configuration.
    pub fn new(strategy: Strategy) -> Self {
        Self::with_config(strategy, PlannerConfig::default())
    }

    /// Reset transient planner state such as engineer shortfall feedback.
    pub fn reset_state(&mut self) {
        self.last_shortfall = [0.0f32; 3];
    }

    /// Create a planner tuned for the reactive actor loop.
    pub fn reactive(strategy: Strategy) -> Self {
        Self::with_config(strategy, PlannerConfig::default())
    }

    /// Create a planner with an explicit configuration.
    pub fn with_config(strategy: Strategy, config: PlannerConfig) -> Self {
        Self {
            strategy,
            config,
            value_net: None,
            default_model: None,
            last_shortfall: [0.0f32; 3],
        }
    }

    /// Create a planner that uses a trained policy bundle.
    pub fn with_value_net(
        strategy: Strategy,
        config: PlannerConfig,
        value_net: PolicyBundle<TrainBackend>,
    ) -> Self {
        Self {
            strategy,
            config,
            value_net: Some(value_net),
            default_model: None,
            last_shortfall: [0.0f32; 3],
        }
    }

    /// Run the planner from `initial_state` until `goal` is completed.
    pub fn plan(
        &mut self,
        units: &Units,
        initial_state: SimulationState,
        goal: &Goal,
    ) -> Result<PlanResult, PlannerError> {
        match self.strategy {
            Strategy::Mcts {
                iterations,
                value_net: _,
                deterministic: _,
            } => {
                let num_edges = num_plan_edges(units, goal).ok_or_else(|| {
                    PlannerError::UnsupportedStrategy("goal has no plan graph".to_string())
                })?;
                let model = match self.value_net.as_ref() {
                    Some(m) => m,
                    None => self.default_model.get_or_insert_with(|| {
                        PolicyBundle::new(&TrainDevice::default(), num_edges)
                    }),
                };
                MctsSearch::new(MctsConfig {
                    iterations,
                    ..MctsConfig::default()
                })
                .search(initial_state, goal, units, &self.config, model)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_parses_mcts() {
        assert_eq!(
            Strategy::from_str("mcts").unwrap(),
            Strategy::Mcts {
                iterations: 100,
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("Mcts:500").unwrap(),
            Strategy::Mcts {
                iterations: 500,
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("mcts:500:gnn").unwrap(),
            Strategy::Mcts {
                iterations: 500,
                value_net: ValueNetKind::Gnn,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("mcts::gnn").unwrap(),
            Strategy::Mcts {
                iterations: 100,
                value_net: ValueNetKind::Gnn,
                deterministic: false,
            }
        );
        assert_eq!(
            Strategy::from_str("mcts:500:gnn:greedy").unwrap(),
            Strategy::Mcts {
                iterations: 500,
                value_net: ValueNetKind::Gnn,
                deterministic: true,
            }
        );
        assert_eq!(
            Strategy::from_str("mcts:greedy").unwrap(),
            Strategy::Mcts {
                iterations: 100,
                value_net: ValueNetKind::Mlp,
                deterministic: true,
            }
        );
    }

    #[test]
    fn strategy_display_includes_value_net() {
        assert_eq!(
            Strategy::Mcts {
                iterations: 200,
                value_net: ValueNetKind::Mlp,
                deterministic: false,
            }
            .to_string(),
            "mcts:200:mlp"
        );
        assert_eq!(
            Strategy::Mcts {
                iterations: 200,
                value_net: ValueNetKind::Gnn,
                deterministic: false,
            }
            .to_string(),
            "mcts:200:gnn"
        );
        assert_eq!(
            Strategy::Mcts {
                iterations: 200,
                value_net: ValueNetKind::Mlp,
                deterministic: true,
            }
            .to_string(),
            "mcts:200:mlp:greedy"
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
            Strategy::from_str("mcts:abc"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
    }
}
