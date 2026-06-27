//! Learned value network for MCTS candidate scoring.
//!
//! The network predicts a scalar score for a `(state, candidate)` pair. It is
//! generic over a Burn backend so the same architecture can be used for
//! inference (`NdArray`) and training (`Autodiff<NdArray>`).

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};

use super::features::{candidate_features, featurize, state_features, FEATURE_COUNT};
use super::pools::Candidate;
use crate::planner::core::PlannerConfig;
use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{UnitKind, Units};

/// A small MLP that scores a candidate action in a given state.
///
/// Input shape: `[batch, FEATURE_COUNT]`.
/// Output shape: `[batch, 1]`.
#[derive(Module, Debug)]
pub struct ValueNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}

impl<B: Backend> ValueNet<B> {
    /// Create a new value network for the fixed feature size.
    ///
    /// Architecture: FEATURE_COUNT -> 128 -> 64 -> 1.
    pub fn new(device: &B::Device) -> Self {
        Self {
            linear1: LinearConfig::new(FEATURE_COUNT, 128).init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(128, 64).init(device),
            output: LinearConfig::new(64, 1).init(device),
        }
    }

    /// Evaluate a batch of feature vectors.
    pub fn forward(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.linear1.forward(features);
        let x = self.activation.forward(x);
        let x = self.linear2.forward(x);
        let x = self.activation.forward(x);
        self.output.forward(x)
    }

    /// Convenience wrapper for a single raw feature vector.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &B::Device) -> f32 {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap()[0]
    }

    /// Score a single candidate in a single state.
    pub fn score(
        &self,
        state: &GraphState,
        candidate: &Candidate,
        goal_id: &UnitKind,
        units: &Units,
        plan: &PlanGraph,
        config: &PlannerConfig,
        device: &B::Device,
    ) -> f32 {
        let features = featurize(state, candidate, goal_id, units, plan, config);
        self.evaluate_single(features, device)
    }

    /// Score a batch of candidates in the same state.
    pub fn score_candidates(
        &self,
        state: &GraphState,
        candidates: &[Candidate],
        goal_id: &UnitKind,
        units: &Units,
        plan: &PlanGraph,
        config: &PlannerConfig,
        device: &B::Device,
    ) -> Vec<(Candidate, f32)> {
        let state_feats = state_features(state, goal_id, units, config);
        candidates
            .iter()
            .map(|c| {
                let mut features = state_feats.clone();
                features.extend(candidate_features(c, plan, units));
                let score = self.evaluate_single(features, device);
                (c.clone(), score)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_net_runs_forward_pass() {
        let device = Default::default();
        let net: ValueNet<burn::backend::NdArray> = ValueNet::new(&device);

        let features = vec![0.0f32; FEATURE_COUNT];
        let value = net.evaluate_single(features, &device);

        // The network is randomly initialized, so we only assert that it
        // returns a finite scalar.
        assert!(
            value.is_finite(),
            "value network should produce a finite scalar"
        );
    }
}
