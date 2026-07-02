//! Trainer for the hierarchical policy networks.

use super::super::{TrainBackend, TrainDevice};
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::features::state_features_with_shortfall;
use crate::planner::mcts::macro_net::{
    clamp_squad, ensure_minimum_squad, masked_argmax, shortfall_from_counts, PolicyBundle,
};
use crate::planner::mcts::policy::{
    execute_action, find_upgrade_edge_idx, upgrade_mask, FACTORY_UPGRADE_OPTIONS,
};
use crate::planner::mcts::selections::{
    assigned_squad_counts, find_upgrade_source, idle_engineer_counts, select_squad_for_edge,
    PlanEdgeIndex,
};
use crate::planner::plan_graph::{EdgeAction, EdgeCategory, PlanGraph};
use crate::planner::search::SimAction;
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    pub(crate) fn evaluate_greedy(
        &self,
        units: &Units,
        goal: &Goal,
        plan: &PlanGraph,
        edge_index: &PlanEdgeIndex,
        planner_config: &PlannerConfig,
    ) -> Option<f64> {
        Self::evaluate_greedy_with_model(
            &self.model,
            units,
            goal,
            plan,
            edge_index,
            planner_config,
            self.config.max_steps,
            self.config.dt,
            &self.device,
        )
    }

    pub(crate) fn evaluate_greedy_with_model(
        model: &PolicyBundle<TrainBackend>,
        units: &Units,
        goal: &Goal,
        _plan: &PlanGraph,
        edge_index: &PlanEdgeIndex,
        planner_config: &PlannerConfig,
        max_steps: usize,
        dt: f64,
        device: &TrainDevice,
    ) -> Option<f64> {
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut shortfall = [0.0f32; 3];

        for _ in 0..max_steps {
            if state.goal_reached(goal) {
                return Some(state.time);
            }

            let macro_features =
                state_features_with_shortfall(&state, units, planner_config, shortfall);

            let upgrade_legal_mask = upgrade_mask(edge_index, &state, units, planner_config);
            let upgrade_idx = if !upgrade_legal_mask.iter().all(|&b| !b) {
                let upgrade_logits = model.evaluate_upgrade(macro_features.clone(), device);
                masked_argmax(&upgrade_logits, &upgrade_legal_mask).unwrap_or(0)
            } else {
                0
            };

            let (_edge_idx, _target_power, desired, builders, action) = if upgrade_idx > 0 {
                let (source_kind, target_kind) = &FACTORY_UPGRADE_OPTIONS[upgrade_idx - 1];
                let edge_idx = match find_upgrade_edge_idx(edge_index, source_kind, target_kind) {
                    Some(idx) => idx,
                    None => {
                        state.tick(units, dt);
                        continue;
                    }
                };
                let edge = match edge_index.get(edge_idx) {
                    Some(e) => e.clone(),
                    None => {
                        state.tick(units, dt);
                        continue;
                    }
                };

                let power_mean = model.evaluate_power(
                    macro_features.clone(),
                    edge_idx,
                    edge_index.len(),
                    device,
                );
                let target_power = power_mean.max(0.0).round();

                let squad_raw = model.evaluate_squad(macro_features, target_power, device);
                let squad_raw_arr = [
                    squad_raw.get(0).copied().unwrap_or(0.0),
                    squad_raw.get(1).copied().unwrap_or(0.0),
                    squad_raw.get(2).copied().unwrap_or(0.0),
                ];

                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(squad_raw_arr, available);
                desired = ensure_minimum_squad(desired, available);

                let builders = select_squad_for_edge(&edge, desired, &state, units);
                if builders.is_empty() {
                    shortfall = shortfall_from_counts(desired, available);
                    state.tick(units, dt);
                    continue;
                }

                let old_node = find_upgrade_source(&state, source_kind)
                    .unwrap_or_else(|| crate::sim::NodeId::new(0));
                let action = SimAction::Upgrade {
                    target_unit_id: target_kind.clone(),
                    old_node,
                    builders: builders.clone(),
                };

                (edge_idx, target_power, desired, builders, action)
            } else {
                let direction_mask = edge_index.legal_category_mask(&state, units, planner_config);
                if direction_mask.iter().all(|&b| !b) {
                    state.tick(units, dt);
                    continue;
                }

                let direction_logits = model.evaluate_direction(macro_features.clone(), device);
                let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
                let category = EdgeCategory::ALL[direction_idx];

                let action_mask =
                    edge_index.legal_mask_for_category(&state, units, planner_config, category);
                if action_mask.iter().all(|&b| !b) {
                    state.tick(units, dt);
                    continue;
                }

                let action_logits = model.evaluate_action(macro_features.clone(), category, device);
                let edge_idx = masked_argmax(&action_logits, &action_mask).unwrap_or(0);

                let edge = match edge_index.get(edge_idx) {
                    Some(e) => e.clone(),
                    None => {
                        state.tick(units, dt);
                        continue;
                    }
                };

                let power_mean = model.evaluate_power(
                    macro_features.clone(),
                    edge_idx,
                    edge_index.len(),
                    device,
                );
                let target_power = power_mean.max(0.0).round();

                let squad_raw = model.evaluate_squad(macro_features, target_power, device);
                let squad_raw_arr = [
                    squad_raw.get(0).copied().unwrap_or(0.0),
                    squad_raw.get(1).copied().unwrap_or(0.0),
                    squad_raw.get(2).copied().unwrap_or(0.0),
                ];

                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(squad_raw_arr, available);
                desired = ensure_minimum_squad(desired, available);

                let builders = select_squad_for_edge(&edge, desired, &state, units);
                if builders.is_empty() {
                    shortfall = shortfall_from_counts(desired, available);
                    state.tick(units, dt);
                    continue;
                }

                let action = match edge.kind {
                    EdgeAction::Build => {
                        if let Some(target_goal) = edge.target_goal() {
                            SimAction::BuildGoal {
                                goal: *target_goal,
                                builders: builders.clone(),
                            }
                        } else {
                            SimAction::Build {
                                unit_id: edge.target_unit().expect("build target unit").clone(),
                                builders: builders.clone(),
                            }
                        }
                    }
                    EdgeAction::Upgrade => SimAction::Upgrade {
                        target_unit_id: edge.target_unit().expect("upgrade target unit").clone(),
                        old_node: find_upgrade_source(
                            &state,
                            edge.source_unit().expect("upgrade source unit"),
                        )
                        .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                        builders: builders.clone(),
                    },
                };

                (edge_idx, target_power, desired, builders, action)
            };

            if execute_action(&mut state, &action, units, dt).is_err() {
                shortfall = shortfall_from_counts(desired, idle_engineer_counts(&state, units));
                state.tick(units, dt);
                continue;
            }

            let assigned_counts = assigned_squad_counts(&state, &builders);
            shortfall = shortfall_from_counts(desired, assigned_counts);
        }

        None
    }
}
