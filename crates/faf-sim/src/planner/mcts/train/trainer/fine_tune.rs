//! Trainer for the hierarchical policy networks.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::BuildTrajectory;
use super::super::math::tensor1d_from_vec;
use super::super::TrainBackend;
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::features::state_features;
use crate::planner::mcts::macro_net::{clamp_squad, ensure_minimum_squad};
use crate::planner::mcts::macro_net::{one_hot, DIRECTION_COUNT, MASK_VALUE, UPGRADE_OPTION_COUNT};
use crate::planner::mcts::policy::{execute_action, upgrade_mask};
use crate::planner::mcts::selections::{
    find_upgrade_source, idle_engineer_counts, select_squad_for_edge, PlanEdgeIndex,
    SelectionOption,
};
use crate::planner::plan_graph::EdgeCategory;
use crate::planner::SimAction;
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    /// Fine-tune the best model on a recorded trajectory using supervised loss.
    pub(crate) fn fine_tune_on_trajectory(
        &mut self,
        trajectory: &BuildTrajectory,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
    ) -> f32 {
        if trajectory.steps.is_empty() {
            return 0.0;
        }

        let plan = units.plan_graph(*goal);
        let edge_index = PlanEdgeIndex::new(&plan);
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss_value = 0.0f32;
        let mut step_count = 0usize;

        for step in &trajectory.steps {
            let mut executable = false;
            for _ in 0..self.config.max_steps {
                let current_mask = edge_index.legal_mask(&state, units);
                if step.edge_index >= current_mask.len() || !current_mask[step.edge_index] {
                    state.tick(units, planner_config.dt);
                    continue;
                }

                let base_features = state_features(&state, units, planner_config);
                let num_edges = edge_index.len();

                let macro_features: Vec<f32> = {
                    let mut v = base_features.clone();
                    v.extend_from_slice(&step.shortfall);
                    v
                };
                let macro_input = tensor1d_from_vec(&macro_features);
                let latent = self.model.latent(macro_input);

                // Upgrade cross-entropy loss (index 0 is "no upgrade").
                let upgrade_legal_mask = upgrade_mask(&edge_index, &state, units);
                let upgrade_logits = self.model.upgrade_logits(latent.clone()).flatten::<1>(0, 1);
                let upgrade_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(
                        upgrade_legal_mask
                            .iter()
                            .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                            .collect(),
                        [UPGRADE_OPTION_COUNT],
                    ),
                    &self.device,
                );
                let masked_upgrade_logits = upgrade_logits + upgrade_mask_tensor;
                let upgrade_log_probs = log_softmax(masked_upgrade_logits, 0);
                let upgrade_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                    TensorData::new(vec![step.upgrade_index as i64], [1]),
                    &self.device,
                );
                let upgrade_ce = upgrade_log_probs.select(0, upgrade_index_tensor).neg();

                // Direction and action losses are only meaningful when no upgrade
                // was chosen for this step.
                let (direction_ce, action_ce) = if step.upgrade_index == 0 {
                    let category = edge_index
                        .get(step.edge_index)
                        .map(|e| e.category())
                        .unwrap_or(EdgeCategory::Goal);
                    let direction_index = category as usize;

                    let direction_logits = self
                        .model
                        .direction_logits(latent.clone())
                        .flatten::<1>(0, 1);
                    let direction_mask = edge_index.legal_category_mask(&state, units);
                    let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(
                            direction_mask
                                .iter()
                                .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                                .collect(),
                            [DIRECTION_COUNT],
                        ),
                        &self.device,
                    );
                    let masked_direction_logits = direction_logits + direction_mask_tensor;
                    let direction_log_probs = log_softmax(masked_direction_logits, 0);
                    let direction_index_tensor =
                        Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                            TensorData::new(vec![direction_index as i64], [1]),
                            &self.device,
                        );
                    let direction_ce = direction_log_probs.select(0, direction_index_tensor).neg();

                    let direction_one_hot = Tensor::<TrainBackend, 2>::from_data(
                        TensorData::new(
                            one_hot(direction_index, DIRECTION_COUNT),
                            [1, DIRECTION_COUNT],
                        ),
                        &self.device,
                    );
                    let action_logits = self
                        .model
                        .action_logits(latent.clone(), direction_one_hot)
                        .flatten::<1>(0, 1);
                    let action_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(
                            current_mask
                                .iter()
                                .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                                .collect(),
                            [num_edges],
                        ),
                        &self.device,
                    );
                    let masked_action_logits = action_logits + action_mask_tensor;
                    let action_log_probs = log_softmax(masked_action_logits, 0);
                    let edge_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                        TensorData::new(vec![step.edge_index as i64], [1]),
                        &self.device,
                    );
                    let action_ce = action_log_probs.select(0, edge_index_tensor).neg();

                    (direction_ce, action_ce)
                } else {
                    let zero = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(vec![0.0f32], [1]),
                        &self.device,
                    );
                    (zero.clone(), zero)
                };

                // Build-power MSE loss.
                let edge_one_hot = Tensor::<TrainBackend, 2>::from_data(
                    TensorData::new(one_hot(step.edge_index, num_edges), [1, num_edges]),
                    &self.device,
                );
                let power_mean = self
                    .model
                    .power_mean(latent.clone(), edge_one_hot)
                    .flatten::<1>(0, 1);
                let target_power = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(vec![step.target_power], [1]),
                    &self.device,
                );
                let power_diff = power_mean - target_power;
                let power_mse = (power_diff.clone() * power_diff).sum();

                // Engineer-squad MSE loss.
                let power_tensor = Tensor::<TrainBackend, 2>::from_data(
                    TensorData::new(vec![step.target_power], [1, 1]),
                    &self.device,
                );
                let squad_means = self
                    .model
                    .squad_means(latent, power_tensor)
                    .flatten::<1>(0, 1);
                let target_squad = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(step.desired_squad.to_vec(), [3]),
                    &self.device,
                );
                let squad_diff = squad_means - target_squad;
                let squad_mse = (squad_diff.clone() * squad_diff).sum();

                let step_loss = direction_ce + action_ce + upgrade_ce + power_mse + squad_mse;
                total_loss_value += step_loss.clone().into_data().as_slice::<f32>().unwrap()[0];
                accumulated_loss = Some(match accumulated_loss {
                    Some(acc) => acc + step_loss,
                    None => step_loss,
                });
                step_count += 1;

                let option = edge_index
                    .to_selection_option(step.edge_index, &state, units)
                    .expect("legal edge must produce a selection option");
                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(step.desired_squad, available);
                desired = ensure_minimum_squad(desired, available);
                let edge = edge_index
                    .get(step.edge_index)
                    .expect("valid edge index")
                    .clone();
                let builders = select_squad_for_edge(&edge, desired, &state, units);

                let action = match option {
                    SelectionOption::Build(target) => SimAction::Build {
                        unit_id: target,
                        builders,
                    },
                    SelectionOption::BuildGoal(goal) => SimAction::BuildGoal { goal, builders },
                    SelectionOption::Upgrade { from, to } => SimAction::Upgrade {
                        target_unit_id: to,
                        old_node: find_upgrade_source(&state, &from)
                            .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                        builders,
                    },
                    SelectionOption::Assist(_) => {
                        unreachable!("assist is not a plan-graph edge")
                    }
                };

                let _ = execute_action(&mut state, &action, units, planner_config.dt);
                executable = true;
                break;
            }

            if !executable {
                break;
            }
        }

        if let Some(loss) = accumulated_loss {
            let grads = loss.backward();
            let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
            self.model =
                self.optimizer
                    .step(self.config.learning_rate.into(), self.model.clone(), grads);
        }

        if step_count == 0 {
            0.0
        } else {
            total_loss_value / step_count as f32
        }
    }
}
