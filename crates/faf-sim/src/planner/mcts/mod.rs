//! Monte Carlo Tree Search planner guided by a learned value network.
//!
//! This module is the entry point for the `Strategy::Mcts` planner. The current
//! implementation is a greedy, MLP-guided one-step planner: it scores every
//! legal candidate action with the value network and commits to the best one.
//! Full UCT tree search will be added on top of the same value network later.

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::plan_graph::PlanGraph;
use crate::planner::search::SearchAction;
use crate::sim::{GraphSimError, GraphState, NodeId};
use crate::units::{TechLevel, UnitKind, Units};

use self::pools::{Candidate, SelectionPools};
use self::train::TrainBackend;
use self::value_net::ValueNet;
use burn::tensor::Device;

pub mod features;
pub mod pools;
pub mod search;
pub mod train;
pub mod value_net;

pub use value_net::ValueNet as MlpValueNet;

/// Run MCTS (currently: greedy MLP scoring) from `initial_state` toward `goal_id`.
///
/// # Arguments
///
/// * `units` - Unified unit knowledge repository.
/// * `initial_state` - Current simulator state.
/// * `goal_id` - Unit kind to build.
/// * `iterations` - Number of MCTS iterations (ignored by the greedy baseline).
/// * `value_net_kind` - Which value-network architecture to use.
/// * `value_net` - Optional trained value network to use for inference.
/// * `config` - Shared planner configuration.
///
/// # Returns
///
/// A [`PlanResult`] containing the selected immediate action.
pub fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    value_net: Option<ValueNet<TrainBackend>>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => mlp_greedy_plan(units, initial_state, goal_id, value_net, config),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}

/// Greedy planner that picks the highest-scoring candidate according to the MLP.
fn mlp_greedy_plan(
    units: &Units,
    state: GraphState,
    goal_id: &UnitKind,
    value_net: Option<ValueNet<TrainBackend>>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let plan = units
        .plan_graph(goal_id)
        .map_err(|e| PlannerError::UnsupportedStrategy(e.to_string()))?;

    let device: Device<TrainBackend> = Default::default();
    let net: ValueNet<TrainBackend> = value_net.unwrap_or_else(|| ValueNet::new(&device));

    let pools = SelectionPools::derive(&plan, &state, units);
    let candidates = pools.candidates();

    if candidates.is_empty() {
        return Ok(plan_result_with_action(state, SearchAction::Wait));
    }

    let scored = net.score_candidates(&state, &candidates, goal_id, units, &plan, config, &device);

    // Pick the highest-scoring candidate that can actually be executed now.
    let mut scored: Vec<_> = scored.into_iter().collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (candidate, _score) in scored {
        if let Some(action) = candidate_to_action(&candidate, &state, units, &plan) {
            return Ok(plan_result_with_action(state, action));
        }
    }

    // Nothing executable right now; wait for builders or resources.
    Ok(plan_result_with_action(state, SearchAction::Wait))
}

/// Convert a candidate into a concrete simulator command if it is executable.
pub(crate) fn candidate_to_action(
    candidate: &Candidate,
    state: &GraphState,
    units: &Units,
    _plan: &PlanGraph,
) -> Option<SearchAction> {
    match candidate {
        Candidate::Build(target) => {
            let builder_id = find_idle_builder(state, units, target)?;
            Some(SearchAction::Build {
                unit_id: target.clone(),
                builder: builder_id,
            })
        }
        Candidate::Upgrade { from, to } => {
            let (old_node, builder_id) = find_upgrade_parts(state, units, from, to)?;
            Some(SearchAction::Upgrade {
                target_unit_id: to.clone(),
                old_node,
                builder: builder_id,
            })
        }
        Candidate::Assist(tier) => {
            let builders = idle_engineers_of_tier(state, units, *tier);
            if builders.is_empty() {
                return None;
            }
            let project_node = best_project_to_assist(state)?;
            Some(SearchAction::Assist {
                project_node,
                builders,
            })
        }
    }
}

/// Find an idle builder node capable of building `target`.
fn find_idle_builder(state: &GraphState, units: &Units, target: &UnitKind) -> Option<NodeId> {
    state
        .idle_builders(units)
        .into_iter()
        .find(|&id| units.can_build(&state.graph[id].unit_id, target))
}

/// Find an active source unit and an idle builder for an upgrade.
fn find_upgrade_parts(
    state: &GraphState,
    units: &Units,
    from: &UnitKind,
    to: &UnitKind,
) -> Option<(NodeId, NodeId)> {
    let recipe = units
        .upgrade_recipes(from)
        .iter()
        .find(|r| r.to == *to)?;

    let old_node = state
        .graph
        .graph
        .node_weights()
        .find(|n| n.is_active() && n.unit_id == *from)
        .map(|n| n.id)?;

    let builder_id = state
        .idle_builders(units)
        .into_iter()
        .find(|&id| recipe.builder_options.contains(&state.graph[id].unit_id))?;

    Some((old_node, builder_id))
}

/// Return all idle engineer nodes of the requested tier.
fn idle_engineers_of_tier(state: &GraphState, units: &Units, tier: TechLevel) -> Vec<NodeId> {
    state
        .idle_builders(units)
        .into_iter()
        .filter(|&id| state.graph[id].unit_id == UnitKind::Engineer(tier))
        .collect()
}

/// Pick the active project that benefits most from assistance.
///
/// Current heuristic: assist the project with the most remaining work.
fn best_project_to_assist(state: &GraphState) -> Option<NodeId> {
    state
        .active_projects
        .iter()
        .max_by(|a, b| {
            a.remaining_work
                .partial_cmp(&b.remaining_work)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|p| p.target_node)
}

/// Apply a [`SearchAction`] to a mutable simulator state.
pub(crate) fn execute_action(
    state: &mut GraphState,
    action: &SearchAction,
    units: &Units,
    dt: f64,
) -> Result<(), GraphSimError> {
    match action {
        SearchAction::Build { unit_id, builder } => {
            state.start_project(unit_id, &[*builder], units)?;
        }
        SearchAction::Upgrade {
            target_unit_id,
            old_node,
            builder,
        } => {
            state.start_upgrade_project(target_unit_id, *old_node, &[*builder], units)?;
        }
        SearchAction::Assist {
            project_node,
            builders,
        } => {
            if builders.is_empty() {
                return Ok(());
            }
            let project_index = state
                .active_projects
                .iter()
                .position(|p| p.target_node == *project_node)
                .ok_or(GraphSimError::ProjectNotFound)?;
            state.assist_project(project_index, builders, units)?;
        }
        SearchAction::Wait => {
            state.tick(units, dt);
        }
    }
    Ok(())
}

/// Build a [`PlanResult`] that commits to a single immediate action.
fn plan_result_with_action(state: GraphState, action: SearchAction) -> PlanResult {
    PlanResult {
        events: Vec::new(),
        completion_time: state.time,
        final_economy: state.economy,
        first_action: Some(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn mlp_plan_selects_build_action_from_acu() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();

        let result = mlp_greedy_plan(&units, state, &goal, None, &config).expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
