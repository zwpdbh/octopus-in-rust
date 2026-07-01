//! Trainer for the hierarchical policy networks.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::Episode;
use super::super::math::{gaussian_log_prob_scalar, gaussian_log_prob_vec, tensor1d_from_vec};
use super::super::TrainBackend;
use crate::planner::mcts::macro_net::{one_hot, DIRECTION_COUNT, MASK_VALUE, UPGRADE_OPTION_COUNT};

use super::Trainer;

impl Trainer {
    pub(crate) fn compute_returns(&mut self, episode: &mut Episode) {
        let step_count = episode.steps.len();
        if step_count == 0 {
            return;
        }

        // Discounted returns from each step, including the terminal bonus at the
        // end of the episode.
        let gamma = self.config.gamma;
        let mut returns = Vec::with_capacity(step_count);
        let mut g = episode.final_reward;
        for step in episode.steps.iter().rev() {
            g = step.step_reward + gamma * g;
            returns.push(g);
        }
        returns.reverse();

        let mean = returns.iter().sum::<f32>() / step_count as f32;
        let std = (returns.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / step_count as f32)
            .sqrt()
            .max(1e-6);

        for (step, ret) in episode.steps.iter_mut().zip(returns) {
            step.return_value = (ret - mean) / std;
        }
    }

    /// Update the hierarchical policy network from one episode using REINFORCE.
    pub(crate) fn update(&mut self, episode: &Episode) -> f32 {
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss = 0.0f32;
        let mut step_count = 0usize;

        for step in &episode.steps {
            let base_features = step.base_features.clone();
            let num_edges = step.action_mask.len();

            let macro_features = {
                let mut v = base_features.clone();
                v.extend_from_slice(&step.shortfall);
                v
            };
            let macro_input = tensor1d_from_vec(&macro_features);
            let latent = self.model.latent(macro_input);

            // Upgrade head (always evaluated; index 0 means "no upgrade").
            let upgrade_logits = self.model.upgrade_logits(latent.clone()).flatten::<1>(0, 1);
            let upgrade_mask: Vec<f32> = step
                .upgrade_mask
                .iter()
                .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                .collect();
            let upgrade_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(upgrade_mask, [UPGRADE_OPTION_COUNT]),
                &self.device,
            );
            let masked_upgrade_logits = upgrade_logits + upgrade_mask_tensor;
            let upgrade_log_probs = log_softmax(masked_upgrade_logits, 0);
            let upgrade_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                TensorData::new(vec![step.upgrade_index as i64], [1]),
                &self.device,
            );
            let upgrade_log_prob = upgrade_log_probs.clone().select(0, upgrade_index_tensor);
            let upgrade_probs = upgrade_log_probs.clone().exp();
            let upgrade_entropy = (upgrade_probs * upgrade_log_probs).neg().sum();

            // Direction and action heads are only part of the decision when no
            // factory upgrade was chosen.
            let (direction_log_prob, action_log_prob, direction_entropy, action_entropy) =
                if step.upgrade_index == 0 {
                    let direction_logits = self
                        .model
                        .direction_logits(latent.clone())
                        .flatten::<1>(0, 1);
                    let direction_mask: Vec<f32> = step
                        .direction_mask
                        .iter()
                        .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                        .collect();
                    let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(direction_mask, [DIRECTION_COUNT]),
                        &self.device,
                    );
                    let masked_direction_logits = direction_logits + direction_mask_tensor;
                    let direction_log_probs = log_softmax(masked_direction_logits, 0);
                    let direction_index_tensor =
                        Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                            TensorData::new(vec![step.direction_index as i64], [1]),
                            &self.device,
                        );
                    let direction_log_prob = direction_log_probs
                        .clone()
                        .select(0, direction_index_tensor);

                    let direction_one_hot = Tensor::<TrainBackend, 2>::from_data(
                        TensorData::new(
                            one_hot(step.direction_index, DIRECTION_COUNT),
                            [1, DIRECTION_COUNT],
                        ),
                        &self.device,
                    );
                    let action_logits = self
                        .model
                        .action_logits(latent.clone(), direction_one_hot)
                        .flatten::<1>(0, 1);
                    let action_mask: Vec<f32> = step
                        .action_mask
                        .iter()
                        .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                        .collect();
                    let action_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(action_mask, [num_edges]),
                        &self.device,
                    );
                    let masked_action_logits = action_logits + action_mask_tensor;
                    let action_log_probs = log_softmax(masked_action_logits, 0);
                    let edge_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                        TensorData::new(vec![step.edge_index as i64], [1]),
                        &self.device,
                    );
                    let action_log_prob = action_log_probs.clone().select(0, edge_index_tensor);

                    let direction_probs = direction_log_probs.clone().exp();
                    let direction_entropy = (direction_probs * direction_log_probs).neg().sum();
                    let action_probs = action_log_probs.clone().exp();
                    let action_entropy = (action_probs * action_log_probs).neg().sum();

                    (
                        direction_log_prob,
                        action_log_prob,
                        direction_entropy,
                        action_entropy,
                    )
                } else {
                    let zero = Tensor::<TrainBackend, 1>::from_data(
                        TensorData::new(vec![0.0f32], [1]),
                        &self.device,
                    );
                    (zero.clone(), zero.clone(), zero.clone(), zero)
                };

            let entropy = direction_entropy + action_entropy + upgrade_entropy;

            // Build-power network.
            let edge_one_hot = Tensor::<TrainBackend, 2>::from_data(
                TensorData::new(one_hot(step.edge_index, num_edges), [1, num_edges]),
                &self.device,
            );
            let power_mean = self
                .model
                .power_mean(latent.clone(), edge_one_hot)
                .flatten::<1>(0, 1);
            let power_log_prob = gaussian_log_prob_scalar(
                power_mean,
                step.target_power,
                self.config.power_std,
                &self.device,
            );

            // Engineer-squad network.
            let power_tensor = Tensor::<TrainBackend, 2>::from_data(
                TensorData::new(vec![step.target_power], [1, 1]),
                &self.device,
            );
            let squad_means = self
                .model
                .squad_means(latent, power_tensor)
                .flatten::<1>(0, 1);
            let squad_log_prob = gaussian_log_prob_vec(
                squad_means,
                &step.desired_squad,
                self.config.squad_std,
                &self.device,
            );

            // REINFORCE only on the discrete decisions (upgrade / direction /
            // action). The continuous power and squad heads are trained with
            // maximum-likelihood on the sampled targets; multiplying their log
            // probabilities by the return would reverse their gradients when
            // returns are negative and cause the continuous predictions to
            // diverge.
            let discrete_log_prob = upgrade_log_prob + direction_log_prob + action_log_prob;
            let return_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![step.return_value], [1]),
                &self.device,
            );
            let policy_loss = discrete_log_prob.neg().mul(return_tensor);
            let continuous_nll = power_log_prob.neg() + squad_log_prob.neg();
            let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);
            let loss = policy_loss + entropy_loss + continuous_nll;

            total_loss += loss.clone().into_data().as_slice::<f32>().unwrap()[0];
            accumulated_loss = Some(match accumulated_loss {
                Some(acc) => acc + loss,
                None => loss,
            });
            step_count += 1;
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
            total_loss / step_count as f32
        }
    }
}
