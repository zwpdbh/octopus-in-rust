//! Greedy planner over the graph-growth model.
//!
//! At each step it expands the current state, scores the successors, and picks
//! the single best one. This implementation uses a very narrow beam search
//! under the hood so that it can still navigate long prerequisite chains
//! (factory / engineer upgrades) without getting stuck.

use faf_units::{DataIndex, Unit};

use crate::planner::beam::BeamPlanner;
use crate::planner::core::{PlanResult, Planner, PlannerError};
use crate::sim::GraphState;

/// Greedy planner over the graph-growth model.
///
/// At each step it expands the current state, scores the successors, and picks
/// the single best one. This is fast but can get stuck in locally optimal
/// choices.
#[derive(Debug, Clone, Copy)]
pub struct GreedyPlanner {
    /// Fixed simulation timestep in seconds.
    pub dt: f64,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
    /// Maximum number of steps before giving up.
    pub max_steps: usize,
}

impl Default for GreedyPlanner {
    fn default() -> Self {
        Self {
            dt: 1.0,
            max_mex_count: 8,
            max_pgen_count: 20,
            max_steps: 20_000,
        }
    }
}

impl Planner for GreedyPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        initial_state: GraphState,
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        // Greedy planning is hard in this domain because long prerequisite
        // chains require committing to expensive investments before any progress
        // is visible. We implement it as beam search with a very narrow beam:
        // this keeps the "fast, greedy" character while retaining just enough
        // lookahead to navigate factory/engineer chains.
        let inner = BeamPlanner {
            beam_width: 3,
            max_depth: self.max_steps,
            dt: self.dt,
            max_mex_count: self.max_mex_count,
            max_pgen_count: self.max_pgen_count,
        };
        inner.plan(index, initial_state, goal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::GraphState;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn greedy_planner_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = GreedyPlanner::default();
        let initial = GraphState::new(&[acu]);
        let result = planner.plan(&index, initial, goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URB1101")),
            "plan should build the goal pgen"
        );
        assert!(result.completion_time > 0.0);
    }
}
