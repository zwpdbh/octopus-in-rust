//! Monte Carlo Tree Search planner guided by a learned value network.
//!
//! This module is the entry point for the `Strategy::Mcts` planner. The current
//! implementation is a stochastic, MLP-guided one-step planner: it scores every
//! legal candidate action with the value network and samples the next action
//! from a softmax distribution over those scores. Full UCT tree search will be
//! added on top of the same value network later.

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::search::SimAction;
use crate::sim::{GraphSimError, GraphState};
use crate::units::{UnitKind, Units};

use self::selections::SelectionPools;
use self::train::TrainBackend;
use self::value_net::ValueNet;
use burn::tensor::Device;
use rand::distributions::WeightedIndex;
use rand::prelude::*;

pub mod features;
pub mod search;
pub mod selections;
pub mod train;
pub mod value_net;

pub use value_net::ValueNet as MlpValueNet;

/// Run MCTS (currently: stochastic MLP policy) from `initial_state` toward `goal_id`.
///
/// # Arguments
///
/// * `units` - Unified unit knowledge repository.
/// * `initial_state` - Current simulator state.
/// * `goal_id` - Unit kind to build.
/// * `iterations` - Number of MCTS iterations (ignored by the one-step policy).
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
        ValueNetKind::Mlp => mlp_policy_plan(units, initial_state, goal_id, value_net, config),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}

/// Temperature for the softmax policy used during inference.
const SAMPLE_TEMPERATURE: f32 = 1.0;

/// Stochastic one-step planner that samples a candidate according to the MLP.
fn mlp_policy_plan(
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
    let candidates = pools.options(&state, units);

    if candidates.is_empty() {
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let scored = net.score_candidates(&state, &candidates, goal_id, units, &plan, config, &device);

    // Keep only candidates that can actually be executed now and sample from them.
    let mut executable = Vec::new();
    let mut scores = Vec::new();
    for (candidate, score) in scored {
        if candidate.to_sim_action(&state, units).is_some() {
            executable.push(candidate);
            scores.push(score);
        }
    }

    if executable.is_empty() {
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let probs = softmax(&scores, SAMPLE_TEMPERATURE);
    let dist = WeightedIndex::new(&probs)
        .map_err(|e| PlannerError::Other(format!("invalid policy distribution: {}", e)))?;
    let mut rng = thread_rng();
    let idx = dist.sample(&mut rng);
    let chosen = &executable[idx];
    let action = chosen
        .to_sim_action(&state, units)
        .expect("executable candidates must map to actions");
    Ok(plan_result_with_action(state, action))
}

/// Numerically stable softmax over raw candidate scores.
fn softmax(scores: &[f32], temperature: f32) -> Vec<f32> {
    let temp = temperature.max(1e-6);
    let max = scores.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let exps: Vec<f32> = scores.iter().map(|s| ((s - max) / temp).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}

/// Apply a [`SimAction`] to a mutable simulator state.
pub(crate) fn execute_action(
    state: &mut GraphState,
    action: &SimAction,
    units: &Units,
    dt: f64,
) -> Result<(), GraphSimError> {
    match action {
        SimAction::Build { unit_id, builder } => {
            state.start_project(unit_id, &[*builder], units)?;
        }
        SimAction::Upgrade {
            target_unit_id,
            old_node,
            builder,
        } => {
            state.start_upgrade_project(target_unit_id, *old_node, &[*builder], units)?;
        }
        SimAction::Assist {
            project_node,
            builders,
        } => {
            if builders.is_empty() {
                return Ok(());
            }
            state.assist_project(*project_node, builders, units)?;
        }
        SimAction::Wait => {
            state.tick(units, dt);
        }
    }
    Ok(())
}

/// Build a [`PlanResult`] that commits to a single immediate action.
fn plan_result_with_action(state: GraphState, action: SimAction) -> PlanResult {
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

        let result =
            mlp_policy_plan(&units, state, &goal, None, &config).expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
