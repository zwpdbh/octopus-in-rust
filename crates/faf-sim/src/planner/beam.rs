//! Beam-search planner over the graph-growth model.
//!
//! It keeps the top-K most promising states each layer and returns the fastest
//! path to the goal unit.

use std::collections::HashSet;

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};

use crate::planner::search::{to_plan_result, visited_key, SearchAction, SearchConfig, VisitedKey};
use crate::sim::GraphState;
use crate::units::{Capability, Units};

/// A node in the beam, tracking the state and the first action taken from the
/// root state on the path that led here.
struct BeamNode {
    state: GraphState,
    first_action: Option<SearchAction>,
}

/// Plan a build order for `goal_id` using beam search.
///
/// `beam_width` controls how many states are kept after each search layer.
/// `config` provides the shared search parameters.
pub(crate) fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &str,
    beam_width: usize,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let goal_chain = units.prerequisite_chain(goal_id, Capability::ACU)?;

    let chain_unit_ids: Vec<String> = goal_chain.iter().map(|(_, id)| id.clone()).collect();

    let search_config = SearchConfig {
        dt: config.dt,
        max_mex_count: config.max_mex_count,
        max_pgen_count: config.max_pgen_count,
    };

    let mut beam: Vec<BeamNode> = vec![BeamNode {
        state: initial_state,
        first_action: None,
    }];
    let mut visited: HashSet<VisitedKey> = HashSet::new();

    for _ in 0..config.max_depth {
        let mut next_beam: Vec<BeamNode> = Vec::new();

        for node in beam {
            let key = visited_key(&node.state);
            if !visited.insert(key) {
                continue;
            }

            for (succ_state, action) in
                search_config.successors(units, &node.state, goal_id, &goal_chain)
            {
                let first_action = node.first_action.clone().unwrap_or(action);
                next_beam.push(BeamNode {
                    state: succ_state,
                    first_action: Some(first_action),
                });
            }
        }

        next_beam.sort_by(|a, b| {
            let fa = a.state.time
                + a.state
                    .estimate_remaining_time_to_goal(goal_id, &chain_unit_ids, units);
            let fb = b.state.time
                + b.state
                    .estimate_remaining_time_to_goal(goal_id, &chain_unit_ids, units);
            fa.total_cmp(&fb)
        });

        beam = next_beam.into_iter().take(beam_width).collect();
        if beam.is_empty() {
            break;
        }

        // If any state in the sorted beam satisfies the goal, return it.
        if let Some(node) = beam.iter().find(|n| n.state.goal_reached(goal_id)) {
            return Ok(to_plan_result(
                node.state.clone(),
                preferred_first_action(&beam),
            ));
        }
    }

    // Final pass: any remaining state may already satisfy the goal.
    if let Some(node) = beam.iter().find(|n| n.state.goal_reached(goal_id)) {
        return Ok(to_plan_result(
            node.state.clone(),
            preferred_first_action(&beam),
        ));
    }

    // No goal state was found. Return the best state we did find so that a
    // reactive wrapper can still take a step forward.
    if let Some(node) = beam.into_iter().next() {
        let first_action = node.first_action.clone();
        return Ok(to_plan_result(node.state, first_action));
    }

    Err(PlannerError::SearchExhausted)
}

/// Choose the first action of the best node in the beam, but avoid `Wait` if
/// there is a concrete build/assist action available. This prevents reactive
/// wrappers from getting stuck in "wait forever" loops.
///
/// The beam is assumed to be sorted by ascending f = time + heuristic score.
fn preferred_first_action(beam: &[BeamNode]) -> Option<SearchAction> {
    for node in beam {
        match &node.first_action {
            Some(SearchAction::Build { .. })
            | Some(SearchAction::Assist { .. })
            | Some(SearchAction::Upgrade { .. }) => {
                return node.first_action.clone();
            }
            _ => {}
        }
    }
    beam.first().and_then(|n| n.first_action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::{Planner, Strategy};
    use crate::sim::GraphState;
    use crate::units::Units;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn beam_planner_reaches_pgen() {
        let units = load_units();

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 20 },
            PlannerConfig {
                dt: 10.0,
                max_depth: 20,
                ..PlannerConfig::default()
            },
        );
        let initial = GraphState::new(&units, &["URL0001"]);
        let result = planner.plan(&units, initial, "URB1101").unwrap();

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
