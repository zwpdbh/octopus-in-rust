//! Greedy planner over the graph-growth model.
//!
//! At each step it expands the current state, scores the successors, and picks
//! the single best one. This implementation is implemented as a narrow beam
//! search (beam width of 3) so that it can still navigate long prerequisite
//! chains (factory / engineer upgrades) without getting stuck.

use crate::planner::beam;
use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::sim::GraphState;
use crate::units::{UnitKind, Units};

/// Greedy beam width.
///
/// This is intentionally small to keep the "fast, greedy" character while
/// retaining just enough lookahead to navigate factory / engineer chains.
const GREEDY_BEAM_WIDTH: usize = 3;

/// Plan a build order for `goal_id` using a narrow greedy beam search.
pub(crate) fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    beam::plan(units, initial_state, goal_id, GREEDY_BEAM_WIDTH, config)
}

#[cfg(test)]
mod tests {
    use crate::planner::core::{Planner, Strategy};
    use crate::sim::GraphState;
    use crate::units::{TechLevel, UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn greedy_planner_reaches_pgen() {
        let units = load_units();

        let planner = Planner::new(Strategy::Greedy);
        let initial = GraphState::new(&units, &[UnitKind::Commander]);
        let result = planner
            .plan(&units, initial, &UnitKind::Pgen(TechLevel::T1))
            .unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Pgen(TechLevel::T1)),
            "plan should build the goal pgen"
        );
        assert!(result.completion_time > 0.0);
    }
}
