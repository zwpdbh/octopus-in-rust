//! Trainer update for the direction-only policy network.

use burn::optim::Optimizer;
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};

use super::super::episode::EpisodeStep;
use super::super::math::tensor1d_from_vec;
use super::super::TrainBackend;
use crate::planner::policy::macro_net::{
    ECO_DIRECTION_COUNT, ECO_DIRECTION_INDICES, GOAL_DIRECTION_INDEX, MASK_VALUE,
};

use super::Trainer;

impl Trainer {
    /// Update both the eco head and the rush head from a single step.
    ///
    /// The eco head is updated with REINFORCE using the rollout-based eco reward.
    /// The rush head is updated with MSE against the real-goal rollout outcome.
    /// Both losses share the backbone and are applied in one gradient step.
    pub(crate) fn update_step(&mut self, step: &EpisodeStep, reward_eco: f32) -> f32 {
        let features = step.base_features.clone();
        let macro_input = tensor1d_from_vec(&features);
        let latent = self.model.latent(macro_input);

        // --- Eco head loss (REINFORCE) ---
        let eco_logits = self.model.eco_logits(latent.clone()).flatten::<1>(0, 1);
        let eco_mask: Vec<f32> = ECO_DIRECTION_INDICES
            .iter()
            .map(|&i| if step.direction_mask[i] { 0.0 } else { MASK_VALUE })
            .collect();
        let eco_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(eco_mask, [ECO_DIRECTION_COUNT]),
            &self.device,
        );
        let masked_eco_logits = eco_logits + eco_mask_tensor;
        let eco_log_probs = log_softmax(masked_eco_logits, 0);

        // Map the chosen EdgeCategory::ALL index to the eco head index.
        let eco_direction_index = ECO_DIRECTION_INDICES
            .iter()
            .position(|&i| i == step.direction_index);
        let loss_eco = if let Some(eco_idx) = eco_direction_index {
            let eco_direction_index_tensor =
                Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                    TensorData::new(vec![eco_idx as i64], [1]),
                    &self.device,
                );
            let eco_log_prob = eco_log_probs.select(0, eco_direction_index_tensor);
            let reward_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![reward_eco], [1]),
                &self.device,
            );
            eco_log_prob.neg().mul(reward_tensor)
        } else {
            // Goal was chosen; the eco head is not updated this step.
            Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![0.0f32], [1]),
                &self.device,
            )
        };

        // --- Rush head loss (MSE against rollout target) ---
        let rush_logit = self.model.rush_logit(latent).flatten::<1>(0, 1);
        let rush_logit_value = rush_logit.clone().into_data().as_slice::<f32>().unwrap()[0];
        let rush_p = sigmoid(rush_logit_value);
        let target = step.rush_target;
        let loss_rush = {
            let diff = rush_p - target;
            Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![diff * diff], [1]),
                &self.device,
            )
        };

        // Combine losses. If the chosen direction was Goal, the eco loss is zero
        // because the eco head was not responsible for that decision.
        let rush_weight_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(vec![self.config.rush_loss_weight], [1]),
            &self.device,
        );
        let weighted_rush_loss = loss_rush.mul(rush_weight_tensor);
        let loss = if step.direction_index == GOAL_DIRECTION_INDEX {
            weighted_rush_loss
        } else {
            loss_eco.add(weighted_rush_loss)
        };
        let loss_value = loss.clone().into_data().as_slice::<f32>().unwrap()[0];

        let grads = loss.backward();
        let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
        self.model = self
            .optimizer
            .step(self.config.learning_rate, self.model.clone(), grads);

        loss_value
    }
}

/// Scalar sigmoid for host-side probability calculation.
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}
