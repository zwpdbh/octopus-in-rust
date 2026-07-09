//! Standalone economy-expansion policy network.
//!
//! This network learns to grow mass income.  It has five outputs corresponding
//! to the eco directions in [`ECO_DIRECTION_INDICES`](crate::planner::policy::macro_net::ECO_DIRECTION_INDICES).

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};

use crate::planner::policy::features::STATE_FEATURE_COUNT;
use crate::planner::policy::macro_net::ECO_DIRECTION_COUNT;

/// Economy-only policy network.
///
/// Input: state features ([`STATE_FEATURE_COUNT`] floats).
/// Output: logits over the five eco directions.
#[derive(Module, Debug)]
pub struct EcoNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    eco_head: Linear<B>,
}

impl<B: Backend> EcoNet<B> {
    /// Create a new eco-only network.
    pub fn new(device: &B::Device) -> Self {
        let backbone_input = STATE_FEATURE_COUNT;
        let backbone_hidden = 128;
        let latent_dim = 64;

        Self {
            backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
            backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
            activation: Relu::new(),
            eco_head: LinearConfig::new(latent_dim, ECO_DIRECTION_COUNT).init(device),
        }
    }

    /// Shared backbone that turns a batch of state feature vectors into latent
    /// vectors.
    pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.backbone1.forward(features);
        let x = self.activation.forward(x);
        let x = self.backbone2.forward(x);
        self.activation.forward(x)
    }

    /// Eco logits from a latent vector.
    pub(crate) fn eco_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.eco_head.forward(latent)
    }

    /// Evaluate the network on a single feature vector.
    ///
    /// Returns logits over the five eco directions.
    pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let tensor =
            Tensor::<B, 2>::from_data(TensorData::new(features, [1, STATE_FEATURE_COUNT]), device);
        let latent = self.latent(tensor);
        let eco = self.eco_logits(latent).flatten::<1>(0, 1);
        eco.into_data().as_slice::<f32>().unwrap().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::policy::train::{TrainBackend, TrainDevice};

    #[test]
    fn eco_net_evaluates_to_five_logits() {
        let device: TrainDevice = Default::default();
        let net = EcoNet::<TrainBackend>::new(&device);
        let features = vec![0.0f32; STATE_FEATURE_COUNT];
        let logits = net.evaluate_direction(features, &device);
        assert_eq!(logits.len(), ECO_DIRECTION_COUNT);
    }
}
