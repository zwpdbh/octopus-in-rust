//! Planner abstraction for build-order generation.
//!
//! A [`Planner`] turns an initial [`GraphState`] into a [`PlanResult`]
//! (timeline + completion time). It dispatches to the MCTS strategy.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::economy::EconomyState;
use crate::planner::mcts;
use crate::sim::{BuildEvent, GraphState};
use crate::units::{UnitKind, Units};

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
    pub first_action: Option<crate::planner::search::SearchAction>,
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
        }
    }
}

impl Error for PlannerError {}

/// Selectable planning algorithm.
///
/// Currently only MCTS is supported. The enum is kept so future strategies can
/// be added without changing the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Monte Carlo Tree Search guided by a learned value network.
    Mcts {
        /// Number of MCTS iterations to run per decision.
        iterations: usize,
    },
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        "mcts"
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        if lower == "mcts" {
            return Ok(Strategy::Mcts { iterations: 100 });
        }
        if let Some(rest) = lower.strip_prefix("mcts:") {
            if let Ok(iterations) = rest.parse::<usize>() {
                return Ok(Strategy::Mcts { iterations });
            }
        }
        Err(PlannerError::UnsupportedStrategy(s.to_string()))
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strategy::Mcts { iterations } => write!(f, "mcts:{}", iterations),
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
}

impl Default for PlannerConfig {
    /// Defaults tuned for MCTS.
    fn default() -> Self {
        Self {
            dt: 10.0,
            max_depth: 400,
            max_mex_count: 8,
            max_pgen_count: 20,
        }
    }
}

/// A planner that dispatches to the MCTS strategy.
#[derive(Debug, Clone, Copy)]
pub struct Planner {
    /// Selected planning strategy.
    pub strategy: Strategy,
    /// Shared search configuration.
    pub config: PlannerConfig,
}

impl Planner {
    /// Create a planner with the default configuration.
    pub fn new(strategy: Strategy) -> Self {
        Self::with_config(strategy, PlannerConfig::default())
    }

    /// Create a planner tuned for the reactive actor loop.
    pub fn reactive(strategy: Strategy) -> Self {
        Self::with_config(strategy, PlannerConfig::default())
    }

    /// Create a planner with an explicit configuration.
    pub fn with_config(strategy: Strategy, config: PlannerConfig) -> Self {
        Self { strategy, config }
    }

    /// Run the planner from `initial_state` until `goal_id` is completed.
    ///
    /// The planner uses `units` to look up unit blueprints and capability
    /// relationships.
    pub fn plan(
        &self,
        units: &Units,
        initial_state: GraphState,
        goal_id: &UnitKind,
    ) -> Result<PlanResult, PlannerError> {
        match self.strategy {
            Strategy::Mcts { iterations } => {
                mcts::plan(units, initial_state, goal_id, iterations, &self.config)
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
            Strategy::Mcts { iterations: 100 }
        );
        assert_eq!(
            Strategy::from_str("Mcts:500").unwrap(),
            Strategy::Mcts { iterations: 500 }
        );
    }

    #[test]
    fn strategy_display_includes_iterations() {
        assert_eq!(Strategy::Mcts { iterations: 200 }.to_string(), "mcts:200");
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
