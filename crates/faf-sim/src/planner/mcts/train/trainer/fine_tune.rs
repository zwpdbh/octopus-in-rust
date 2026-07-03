//! Supervised fine-tuning for the direction-only policy network.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::BuildTrajectory;
use super::super::math::tensor1d_from_vec;
use super::super::TrainBackend;
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::features::state_features;
use crate::planner::mcts::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::mcts::macro_net::{DIRECTION_COUNT, MASK_VALUE};
use crate::planner::mcts::policy::execute_action;
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
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

        let plan = build_plan_graph(units, *goal);
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss_value = 0.0f32;
        let mut step_count = 0usize;

        for step in &trajectory.steps {
            let mut executable = false;
            for _ in 0..self.config.max_steps {
                let direction_mask =
                    legal_direction_mask(&state, units, planner_config, goal, &plan);
                if !direction_mask[step.direction_index] {
                    state.tick(units, planner_config.dt);
                    continue;
                }

                let base_features = state_features(&state, units, planner_config);
                let macro_features: Vec<f32> = {
                    let mut v = base_features.clone();
                    v.extend_from_slice(&step.shortfall);
                    v
                };
                let macro_input = tensor1d_from_vec(&macro_features);
                let latent = self.model.latent(macro_input);

                let direction_logits = self.model.direction_logits(latent).flatten::<1>(0, 1);
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
                        TensorData::new(vec![step.direction_index as i64], [1]),
                        &self.device,
                    );
                let direction_ce = direction_log_probs.select(0, direction_index_tensor).neg();

                total_loss_value += direction_ce.clone().into_data().as_slice::<f32>().unwrap()[0];
                accumulated_loss = Some(match accumulated_loss {
                    Some(acc) => acc + direction_ce,
                    None => direction_ce,
                });
                step_count += 1;

                let direction = EdgeCategory::ALL[step.direction_index];
                let action =
                    direction_to_action(direction, &state, units, planner_config, goal, &plan);
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

/// Build a boolean mask over [`EdgeCategory::ALL`] indicating which directions
/// have at least one legal concrete action right now.
fn legal_direction_mask(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    goal: &Goal,
    plan: &PlanGraph,
) -> Vec<bool> {
    EdgeCategory::ALL
        .iter()
        .map(|&d| is_direction_legal(d, state, units, config, goal, plan))
        .collect()
}
