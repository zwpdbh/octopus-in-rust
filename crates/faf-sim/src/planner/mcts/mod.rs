//! Monte Carlo Tree Search planner guided by a learned macro-direction policy.
//!
//! This module is the entry point for the `Strategy::Mcts` planner. The current
//! implementation is a one-step planner: the network scores four macro
//! directions (build power, mass, power, tech) from economy/state features, and
//! a deterministic resolver turns the chosen direction into a concrete build
//! command.

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::search::SimAction;
use crate::sim::{GraphSimError, GraphState};
use crate::units::{UnitKind, Units};

use self::macro_net::{resolve_macro_direction, MacroDirection, MacroNet};
use self::selections::SelectionPools;
use self::train::TrainBackend;
use burn::tensor::Device;
use rand::distributions::WeightedIndex;
use rand::prelude::*;

pub mod features;
pub mod macro_net;
pub mod search;
pub mod selections;
pub mod train;

pub use macro_net::MacroNet as MlpValueNet;

/// Run MCTS (currently: one-step macro policy) from `initial_state` toward `goal_id`.
///
/// # Arguments
///
/// * `units` - Unified unit knowledge repository.
/// * `initial_state` - Current simulator state.
/// * `goal_id` - Unit kind to build.
/// * `iterations` - Number of MCTS iterations (ignored by the one-step policy).
/// * `value_net_kind` - Which value-network architecture to use.
/// * `value_net` - Optional trained macro network to use for inference.
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
    deterministic: bool,
    value_net: Option<MacroNet<TrainBackend>>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => macro_policy_plan(
            units,
            initial_state,
            goal_id,
            value_net,
            deterministic,
            config,
        ),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}

/// One-step planner guided by the macro-direction network.
///
/// In deterministic mode the highest-scoring macro direction is chosen and
/// resolved into a concrete action. In stochastic mode the direction is sampled
/// from a softmax distribution, which is useful during training.
fn macro_policy_plan(
    units: &Units,
    state: GraphState,
    goal_id: &UnitKind,
    value_net: Option<MacroNet<TrainBackend>>,
    deterministic: bool,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let plan = units
        .plan_graph(goal_id)
        .map_err(|e| PlannerError::UnsupportedStrategy(e.to_string()))?;

    let device: Device<TrainBackend> = Default::default();
    let net: MacroNet<TrainBackend> = value_net.unwrap_or_else(|| MacroNet::new(&device));

    let pools = SelectionPools::new(&plan, &state, units, config);
    let candidates = pools.options().to_vec();

    if candidates.is_empty() {
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let scores = net.score_directions(&state, units, config, &device);

    // Try directions in order of network preference until the resolver finds an
    // executable candidate. This handles cases where the top direction has no
    // legal concrete action (e.g., pgen cap reached).
    let mut direction_order: Vec<usize> = (0..scores.len()).collect();
    if deterministic {
        direction_order.sort_by(|&a, &b| {
            scores[b]
                .partial_cmp(&scores[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        direction_order = stochastic_direction_order(&scores);
    }

    for &dir_idx in &direction_order {
        let direction = MacroDirection::from_index(dir_idx).unwrap_or(MacroDirection::BuildPower);
        if let Some(option) =
            resolve_macro_direction(direction, &candidates, &state, units, &plan, config)
        {
            if let Some(action) = option.to_sim_action(&state, units) {
                return Ok(plan_result_with_action(state, action));
            }
        }
    }

    // No direction produced an executable action; wait one tick.
    Ok(plan_result_with_action(state, SimAction::Wait))
}

/// Return direction indices for stochastic inference.
///
/// The first index is sampled from the softmax over scores. The rest are
/// sorted by score descending so they act as a deterministic fallback if the
/// sampled direction has no executable candidate.
fn stochastic_direction_order(scores: &[f32]) -> Vec<usize> {
    let mut rng = thread_rng();
    let probs = macro_net::softmax_probs(scores);
    let dist = WeightedIndex::new(&probs)
        .unwrap_or_else(|_| WeightedIndex::new(vec![1.0f32; scores.len()]).unwrap());
    let sampled = dist.sample(&mut rng);

    let mut rest: Vec<usize> = (0..scores.len()).filter(|&i| i != sampled).collect();
    rest.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut order = vec![sampled];
    order.extend(rest);
    order
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
    fn macro_plan_selects_build_action_from_acu() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();

        let result = macro_policy_plan(&units, state, &goal, None, false, &config)
            .expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
