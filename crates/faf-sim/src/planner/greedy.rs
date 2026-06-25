//! Greedy planner over the graph-growth model.
//!
//! At each step it expands the current state, scores the successors, and picks
//! the single best one. This implementation is implemented as a narrow beam
//! search (beam width of 3) so that it can still navigate long prerequisite
//! chains (factory / engineer upgrades) without getting stuck.

use faf_units::{DataIndex, Unit};

use crate::planner::beam;
use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::sim::GraphState;

/// Greedy beam width.
///
/// This is intentionally small to keep the "fast, greedy" character while
/// retaining just enough lookahead to navigate factory / engineer chains.
const GREEDY_BEAM_WIDTH: usize = 3;

/// Plan a build order for `goal` using a narrow greedy beam search.
pub(crate) fn plan(
    index: &DataIndex,
    initial_state: GraphState,
    goal: &Unit,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    beam::plan(index, initial_state, goal, GREEDY_BEAM_WIDTH, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::{Planner, Strategy};
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

        let planner = Planner::new(Strategy::Greedy);
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
