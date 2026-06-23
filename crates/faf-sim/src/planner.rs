//! Planner abstraction for build-order generation.
//!
//! A `Planner` turns a starting state and a goal unit into a `PlanResult`
//! (timeline + completion time). New algorithms are added by implementing
//! `Planner` and registering the strategy in `build_planner`.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use faf_units::{DataIndex, Unit};

use crate::beam_search::BeamSearchPlanner;
use crate::economy::EconomyState;
use crate::graph_planner::GraphPlanner;
use crate::greedy::StateMachinePolicy;
use crate::sim::BuildEvent;
use crate::simulator::HeuristicSimulator;
use crate::tech_graph::{TechGraph, TechGraphError};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Greedy state-machine policy (default).
    Greedy,
    /// Beam-search planner.
    Beam,
    /// Graph-growth planner with indivisible builders and concurrent projects.
    Graph,
}

impl Strategy {
    /// Human-readable strategy name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Strategy::Greedy => "greedy",
            Strategy::Beam => "beam",
            Strategy::Graph => "graph",
        }
    }
}

impl FromStr for Strategy {
    type Err = PlannerError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "greedy" => Ok(Strategy::Greedy),
            "beam" => Ok(Strategy::Beam),
            "graph" => Ok(Strategy::Graph),
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
        Strategy::Beam => Ok(Box::new(BeamSearchPlanner::default())),
        Strategy::Graph => Ok(Box::new(GraphPlanner::default())),
    }
}

/// Algorithm that produces a build order/timeline for a goal unit.
pub trait Planner {
    /// Run the planner from `starting_units` until `goal` is completed.
    ///
    /// `graph` is provided for algorithms that need dependency/capability
    /// information; the greedy planner builds its own graph internally.
    fn plan(
        &self,
        index: &DataIndex,
        graph: &TechGraph,
        starting_units: &[&Unit],
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError>;
}

/// Greedy planner backed by `HeuristicSimulator` and `StateMachinePolicy`.
#[derive(Debug, Clone, Copy)]
pub struct GreedyPlanner {
    /// Policy used to choose the next project on each tick.
    pub policy: StateMachinePolicy,
    /// Fixed simulation timestep in seconds.
    pub tick_interval: f64,
}

impl Default for GreedyPlanner {
    fn default() -> Self {
        Self {
            policy: StateMachinePolicy::default(),
            tick_interval: 1.0,
        }
    }
}

impl Planner for GreedyPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        _graph: &TechGraph,
        starting_units: &[&Unit],
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        let starting: Vec<&Unit> = starting_units.to_vec();
        let mut sim =
            HeuristicSimulator::new(index, starting, goal, self.policy, self.tick_interval);

        let goal_event = sim.run().ok_or(PlannerError::SimulationFailed)?;

        Ok(PlanResult {
            events: sim.events.clone(),
            completion_time: goal_event.time,
            final_economy: sim.state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tech_graph::TechGraph;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn greedy_planner_reaches_monkeylord() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URL0402").expect("Monkeylord exists");

        let planner = GreedyPlanner::default();
        let result = planner.plan(&index, &graph, &[acu], goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URL0402")),
            "timeline should include the goal unit"
        );
        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URB1101")),
            "timeline should include at least one T1 power generator"
        );
        assert!(result.completion_time > 0.0);
    }

    #[test]
    fn strategy_parses_greedy() {
        assert_eq!(Strategy::from_str("greedy").unwrap(), Strategy::Greedy);
        assert_eq!(Strategy::from_str("Greedy").unwrap(), Strategy::Greedy);
    }

    #[test]
    fn unknown_strategy_errors() {
        assert!(matches!(
            Strategy::from_str("astar"),
            Err(PlannerError::UnsupportedStrategy(_))
        ));
    }
}
