//! Learned value network for MCTS leaf evaluation.
//!
//! The network predicts a scalar value for a `GraphState`. In this scaffold it
//! is a tiny MLP with the `NdArray` backend so the rest of `faf-sim` does not
//! need to be generic over a Burn backend.

use burn::backend::NdArray;
use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::{Tensor, TensorData};

/// Fixed backend for the value network.
///
/// Using a concrete backend keeps the public API simple. Later we can make this
/// configurable via feature flags (e.g., `wgpu` or `cuda`) without changing
/// call sites.
pub type Backend = NdArray;

/// Device on which the network runs.
pub type Device = burn::tensor::Device<Backend>;

/// A small MLP that estimates the value of a FAF state.
///
/// Input shape: `[batch, feature_count]`.
/// Output shape: `[batch, 1]`.
#[derive(Module, Debug, Clone)]
pub struct ValueNet {
    linear1: Linear<Backend>,
    activation: Relu,
    linear2: Linear<Backend>,
    output: Linear<Backend>,
}

impl ValueNet {
    /// Create a new value network with the given input feature count.
    ///
    /// Architecture: feature_count -> 128 -> 64 -> 1.
    pub fn new(feature_count: usize, device: &Device) -> Self {
        Self {
            linear1: LinearConfig::new(feature_count, 128).init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(128, 64).init(device),
            output: LinearConfig::new(64, 1).init(device),
        }
    }

    /// Evaluate a batch of states.
    ///
    /// # Arguments
    ///
    /// * `features` - A `[batch, feature_count]` tensor of state features.
    ///
    /// # Returns
    ///
    /// A `[batch, 1]` tensor with the predicted value of each state.
    pub fn forward(&self, features: Tensor<Backend, 2>) -> Tensor<Backend, 2> {
        let x = self.linear1.forward(features);
        let x = self.activation.forward(x);
        let x = self.linear2.forward(x);
        let x = self.activation.forward(x);
        self.output.forward(x)
    }

    /// Convenience wrapper for a single state feature vector.
    ///
    /// # Arguments
    ///
    /// * `features` - A `Vec<f32>` of length `feature_count`.
    /// * `device` - The device to run inference on.
    ///
    /// # Returns
    ///
    /// A scalar value estimate.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &Device) -> f32 {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap()[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_net_runs_forward_pass() {
        let device = Default::default();
        let net = ValueNet::new(8, &device);

        // A dummy state feature vector.
        let features = vec![0.0f32; 8];
        let value = net.evaluate_single(features, &device);

        // The network is randomly initialized, so we only assert that it
        // returns a finite scalar.
        assert!(
            value.is_finite(),
            "value network should produce a finite scalar"
        );
    }
}
