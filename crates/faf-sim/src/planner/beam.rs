//! Beam-search planner over the graph-growth model.
//!
//! It keeps the top-K most promising states each layer and returns the fastest
//! path to the goal unit.

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::planner::heuristic::score;
use crate::planner::search::{
    goals_reached, to_plan_result, visited_key, SearchAction, SearchConfig, VisitedKey,
};
use crate::sim::GraphState;
use crate::tech_graph::{Capability, TechGraph};

/// A node in the beam, tracking the state and the first action taken from the
/// root state on the path that led here.
struct BeamNode {
    state: GraphState,
    first_action: Option<SearchAction>,
}

/// Plan a build order for `goal` using beam search.
///
/// `beam_width` controls how many states are kept after each search layer.
/// `config` provides the shared search parameters.
pub(crate) fn plan(
    index: &DataIndex,
    initial_state: GraphState,
    goal: &Unit,
    beam_width: usize,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let tech_graph = TechGraph::new(index);
    plan_goals(
        index,
        &tech_graph,
        initial_state,
        &[goal],
        beam_width,
        config,
    )
}

/// Plan a build order for one or more goal units.
pub(crate) fn plan_goals(
    index: &DataIndex,
    tech_graph: &TechGraph,
    initial_state: GraphState,
    goals: &[&Unit],
    beam_width: usize,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    if goals.is_empty() {
        return Err(PlannerError::SearchExhausted);
    }

    let mut goal_chains: Vec<Vec<(Capability, String)>> = Vec::with_capacity(goals.len());
    for goal in goals {
        let chain = tech_graph.prerequisite_chain(&goal.id, Capability::ACU)?;
        goal_chains.push(chain);
    }

    let mut chain_unit_ids: Vec<String> = goal_chains
        .iter()
        .flat_map(|chain| chain.iter().map(|(_, id)| id.clone()))
        .collect();
    chain_unit_ids.sort();
    chain_unit_ids.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

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
                search_config.successors(index, tech_graph, &node.state, goals, &goal_chains)
            {
                let first_action = node.first_action.clone().unwrap_or(action);
                next_beam.push(BeamNode {
                    state: succ_state,
                    first_action: Some(first_action),
                });
            }
        }

        next_beam.sort_by(|a, b| {
            let fa = a.state.time + score(&a.state, goals, &chain_unit_ids, index);
            let fb = b.state.time + score(&b.state, goals, &chain_unit_ids, index);
            fa.total_cmp(&fb)
        });

        beam = next_beam.into_iter().take(beam_width).collect();
        if beam.is_empty() {
            break;
        }

        // If any state in the sorted beam satisfies the goals, return it.
        if let Some(node) = beam.iter().find(|n| goals_reached(&n.state, goals)) {
            return Ok(to_plan_result(
                node.state.clone(),
                preferred_first_action(&beam),
            ));
        }
    }

    // Final pass: any remaining state may already satisfy the goals.
    if let Some(node) = beam.iter().find(|n| goals_reached(&n.state, goals)) {
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
            Some(SearchAction::Build { .. }) | Some(SearchAction::Assist { .. }) => {
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

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn beam_planner_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 20 },
            PlannerConfig {
                dt: 10.0,
                max_depth: 20,
                ..PlannerConfig::default()
            },
        );
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
