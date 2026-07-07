//! Trainer update for the direction-only policy network.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::EpisodeStep;
use super::super::math::tensor1d_from_vec;
use super::super::TrainBackend;
use crate::planner::policy::macro_net::{DIRECTION_COUNT, MASK_VALUE};

use super::Trainer;

impl Trainer {
    /// Update the policy from a single step using online REINFORCE.
    ///
    /// The loss is `-log π(direction | state) * reward`. A positive reward
    /// increases the probability of the chosen direction; a negative reward
    /// decreases it. The gradient step is applied immediately.
    pub(crate) fn update_step(&mut self, step: &EpisodeStep, reward: f32) -> f32 {
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
        let direction_log_prob = direction_log_probs.select(0, direction_index_tensor);

        let reward_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(vec![reward], [1]),
            &self.device,
        );
        let loss = direction_log_prob.neg().mul(reward_tensor);
        let loss_value = loss.clone().into_data().as_slice::<f32>().unwrap()[0];

        let grads = loss.backward();
        let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
        self.model = self
            .optimizer
            .step(self.config.learning_rate, self.model.clone(), grads);

        loss_value
    }
}
