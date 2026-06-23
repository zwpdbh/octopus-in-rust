//! Planner abstraction for build-order generation.
//!
//! A `Planner` turns an initial [`GraphState`] into a [`PlanResult`]
//! (timeline + completion time). All planners in this crate operate on the
//! graph-growth model implemented in [`crate::graph_sim`]; they differ only in
//! how they search the space of possible build schedules.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use faf_units::{DataIndex, Unit};

use crate::economy::EconomyState;
use crate::graph_planner::{BeamPlanner, GreedyPlanner};
use crate::graph_sim::GraphState;
use crate::sim::BuildEvent;
use crate::tech_graph::TechGraphError;

/// Result of running a planner to completion.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    /// Completion events in chronological order.
    pub events: Vec<BuildEvent>,
    /// In-game seconds when the goal unit finished.
    pub completion_time: f64,
    /// Economy state at the end of the plan.
    pub final_economy: EconomyState,
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
    /// Dependency/capability graph query failed.
    TechGraph(TechGraphError),
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
            PlannerError::TechGraph(e) => write!(f, "tech graph error: {}", e),
        }
    }
}

impl Error for PlannerError {}

impl From<TechGraphError> for PlannerError {
    fn from(e: TechGraphError) -> Self {
        PlannerError::TechGraph(e)
    }
}

/// Selectable planning algorithm.
///
/// Both variants use the graph-growth model from [`crate::graph_sim`]. The
/// difference is the search algorithm applied to that model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Greedy: pick the single best successor state at each step.
    Greedy,
    /// Beam search: keep the top-K most promising states each layer.
    Beam,
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Strategy::Greedy => "greedy",
            Strategy::Beam => "beam",
        }
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "greedy" => Ok(Strategy::Greedy),
            "beam" => Ok(Strategy::Beam),
            other => Err(PlannerError::UnsupportedStrategy(other.to_string())),
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Build a planner instance for the requested strategy.
pub fn build_planner(strategy: Strategy) -> Result<Box<dyn Planner>, PlannerError> {
    match strategy {
        Strategy::Greedy => Ok(Box::new(GreedyPlanner::default())),
        Strategy::Beam => Ok(Box::new(BeamPlanner::default())),
    }
}

/// Algorithm that produces a build order/timeline for a goal unit.
pub trait Planner {
    /// Run the planner from `initial_state` until `goal` is completed.
    ///
    /// The planner uses `index` to look up unit blueprints and builds its own
    /// internal [`crate::tech_graph::TechGraph`] for capability checks.
    fn plan(
        &self,
        index: &DataIndex,
        initial_state: GraphState,
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_sim::GraphState;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn greedy_planner_reaches_monkeylord() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URL0402").expect("Monkeylord exists");

        let planner = GreedyPlanner::default();
        let initial = GraphState::new(&[acu]);
        let result = planner.plan(&index, initial, goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URL0402")),
            "timeline should include the goal unit"
        );
        assert!(result.completion_time > 0.0);
    }

    #[test]
    fn beam_planner_reaches_monkeylord() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URL0402").expect("Monkeylord exists");

        let planner = BeamPlanner {
            beam_width: 50,
            max_depth: 400,
            dt: 10.0,
            ..Default::default()
        };
        let initial = GraphState::new(&[acu]);
        let result = planner.plan(&index, initial, goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URL0402")),
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
        assert_eq!(Strategy::from_str("beam").unwrap(), Strategy::Beam);
        assert_eq!(Strategy::from_str("Beam").unwrap(), Strategy::Beam);
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
    }
}
