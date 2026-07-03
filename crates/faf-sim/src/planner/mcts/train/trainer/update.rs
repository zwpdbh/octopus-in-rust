//! Trainer update for the direction-only policy network.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::Episode;
use super::super::math::tensor1d_from_vec;
use super::super::TrainBackend;
use crate::planner::mcts::macro_net::{DIRECTION_COUNT, MASK_VALUE};

use super::Trainer;

impl Trainer {
    pub(crate) fn compute_returns(&mut self, episode: &mut Episode) {
        let step_count = episode.steps.len();
        if step_count == 0 {
            return;
        }

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

    /// Update the direction-only policy network from one episode using REINFORCE.
    pub(crate) fn update(&mut self, episode: &Episode) -> f32 {
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss = 0.0f32;
        let mut step_count = 0usize;

        for step in &episode.steps {
            let features = step.base_features.clone();
            let macro_input = tensor1d_from_vec(&features);
            let latent = self.model.latent(macro_input);

            let direction_logits = self.model.direction_logits(latent).flatten::<1>(0, 1);
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
            let direction_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                TensorData::new(vec![step.direction_index as i64], [1]),
                &self.device,
            );
            let direction_log_prob = direction_log_probs
                .clone()
                .select(0, direction_index_tensor);

            let direction_probs = direction_log_probs.clone().exp();
            let direction_entropy = (direction_probs * direction_log_probs).neg().sum();

            let entropy = direction_entropy;
            let discrete_log_prob = direction_log_prob;
            let return_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![step.return_value], [1]),
                &self.device,
            );
            let policy_loss = discrete_log_prob.neg().mul(return_tensor);
            let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);
            let loss = policy_loss + entropy_loss;

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
            self.model = self
                .optimizer
                .step(self.config.learning_rate, self.model.clone(), grads);
        }

        if step_count == 0 {
            0.0
        } else {
            total_loss / step_count as f32
        }
    }
}
