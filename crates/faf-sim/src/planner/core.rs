//! Planner abstraction for build-order generation.
//!
//! A [`Planner`] turns an initial [`GraphState`] into a [`PlanResult`]
//! (timeline + completion time). It dispatches to strategy-specific pure
//! functions based on its [`Strategy`] enum value.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use crate::economy::EconomyState;
use crate::planner::{beam, greedy, mcts};
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
/// All variants use the graph-growth model from [`crate::sim`]. The difference
/// is the search algorithm applied to that model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Greedy: pick the single best successor state at each step.
    Greedy,
    /// Beam search: keep the top-K most promising states each layer.
    Beam {
        /// Number of states kept after each search layer.
        beam_width: usize,
    },
    /// Monte Carlo Tree Search guided by a learned value network.
    Mcts {
        /// Number of MCTS iterations to run per decision.
        iterations: usize,
    },
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Strategy::Greedy => "greedy",
            Strategy::Beam { .. } => "beam",
            Strategy::Mcts { .. } => "mcts",
        }
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        if lower == "greedy" {
            return Ok(Strategy::Greedy);
        }
        if lower == "beam" {
            return Ok(Strategy::Beam { beam_width: 50 });
        }
        if let Some(rest) = lower.strip_prefix("beam:") {
            if let Ok(width) = rest.parse::<usize>() {
                return Ok(Strategy::Beam { beam_width: width });
            }
        }
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
            Strategy::Greedy => write!(f, "greedy"),
            Strategy::Beam { beam_width } => write!(f, "beam:{}", beam_width),
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
    /// Defaults tuned for beam search.
    fn default() -> Self {
        Self {
            dt: 10.0,
            max_depth: 400,
            max_mex_count: 8,
            max_pgen_count: 20,
        }
    }
}

/// A planner that dispatches to a concrete strategy via enum matching.
#[derive(Debug, Clone, Copy)]
pub struct Planner {
    /// Selected planning strategy.
    pub strategy: Strategy,
    /// Shared search configuration.
    pub config: PlannerConfig,
}

impl Planner {
    /// Create a planner with strategy-specific default configuration.
    pub fn new(strategy: Strategy) -> Self {
        let config = match strategy {
            Strategy::Greedy => PlannerConfig {
                dt: 1.0,
                max_depth: 20_000,
                max_mex_count: 8,
                max_pgen_count: 20,
            },
            Strategy::Beam { .. } => PlannerConfig::default(),
            Strategy::Mcts { .. } => PlannerConfig::default(),
        };
        Self { strategy, config }
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
            Strategy::Greedy => greedy::plan(units, initial_state, goal_id, &self.config),
            Strategy::Beam { beam_width } => {
                beam::plan(units, initial_state, goal_id, beam_width, &self.config)
            }
            Strategy::Mcts { iterations } => {
                mcts::plan(units, initial_state, goal_id, iterations, &self.config)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::GraphState;
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn greedy_planner_reaches_monkeylord() {
        let units = load_units();

        let planner = Planner::new(Strategy::Greedy);
        let initial = GraphState::new(&units, &[UnitKind::Commander]);
        let goal = UnitKind::Unique(UnitId("URL0402".to_string()));
        let result = planner.plan(&units, initial, &goal).unwrap();

        assert!(
            result.events.iter().any(|e| e.unit_id == goal),
            "timeline should include the goal unit"
        );
        assert!(result.completion_time > 0.0);
    }

    #[test]
    fn beam_planner_reaches_monkeylord() {
        let units = load_units();

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 50 },
            PlannerConfig {
                dt: 10.0,
                max_depth: 400,
                ..PlannerConfig::default()
            },
        );
        let initial = GraphState::new(&units, &[UnitKind::Commander]);
        let goal = UnitKind::Unique(UnitId("URL0402".to_string()));
        let result = planner.plan(&units, initial, &goal).unwrap();

        assert!(
            result.events.iter().any(|e| e.unit_id == goal),
            "timeline should include the goal unit"
        );
        assert!(result.completion_time > 0.0);
        assert!(
            result.completion_time < 9000.0,
            "beam planner should beat the 147-minute greedy baseline"
        );
    }

    #[test]
    fn strategy_parses_greedy() {
        assert_eq!(Strategy::from_str("greedy").unwrap(), Strategy::Greedy);
        assert_eq!(Strategy::from_str("Greedy").unwrap(), Strategy::Greedy);
    }

    #[test]
    fn strategy_parses_beam() {
        assert_eq!(
            Strategy::from_str("beam").unwrap(),
            Strategy::Beam { beam_width: 50 }
        );
        assert_eq!(
            Strategy::from_str("Beam").unwrap(),
            Strategy::Beam { beam_width: 50 }
        );
    }

    #[test]
    fn strategy_parses_beam_with_width() {
        assert_eq!(
            Strategy::from_str("beam:20").unwrap(),
            Strategy::Beam { beam_width: 20 }
        );
        assert_eq!(
            Strategy::from_str("Beam:100").unwrap(),
            Strategy::Beam { beam_width: 100 }
        );
    }

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
    fn strategy_display_includes_beam_width() {
        assert_eq!(Strategy::Greedy.to_string(), "greedy");
        assert_eq!(Strategy::Beam { beam_width: 20 }.to_string(), "beam:20");
        assert_eq!(Strategy::Mcts { iterations: 200 }.to_string(), "mcts:200");
    }

    #[test]
    fn unknown_strategy_errors() {
        assert!(matches!(
            Strategy::from_str("astar"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
        assert!(matches!(
            Strategy::from_str("graph"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
        assert!(matches!(
            Strategy::from_str("beam:abc"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
    }
}
