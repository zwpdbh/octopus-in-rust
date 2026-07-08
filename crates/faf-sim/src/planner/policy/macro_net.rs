//! Learned direction-only policy network.
//!
//! The network now chooses only a high-level strategic direction. A separate
//! heuristic layer turns that direction into a concrete build/upgrade action.
//!
//! This keeps the parts of the decision that are hard to get right from
//! heuristics (when to switch economic focus, when to tech, when to start the
//! goal) in the learned network, while moving concrete target selection,
//! build-power assignment, and engineer-squad selection to cheap deterministic
//! rules.

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};
use rand::distr::weighted::WeightedIndex;
use rand::distr::Distribution;
use rand::rngs::ThreadRng;

use super::features::STATE_FEATURE_COUNT;

/// Number of strategic eco directions the eco head can choose from.
///
/// The eco directions are `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`,
/// `IncreaseEnergyStorage`, and `UpgradeTech`.  The `Goal` direction is handled
/// by the separate rush head, so it is not part of this count.
pub const ECO_DIRECTION_COUNT: usize = 5;

/// Indices into [`EdgeCategory::ALL`] that map to eco directions.
///
/// Order matches the output of the eco head:
/// `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `IncreaseEnergyStorage`,
/// `UpgradeTech`.
pub const ECO_DIRECTION_INDICES: [usize; 5] = [0, 1, 2, 3, 5];

/// Index into [`EdgeCategory::ALL`] for the `Goal` direction.
pub const GOAL_DIRECTION_INDEX: usize = 4;

/// Mask value that makes a logit numerically irrelevant after softmax.
pub(crate) const MASK_VALUE: f32 = -1e9;

/// Re-export the old name for code that has not been updated yet.
#[allow(dead_code)]
pub const DIRECTION_COUNT: usize = ECO_DIRECTION_COUNT;

/// Direction-only policy network with a separate rush-readiness head.
///
/// Input: state features ([`STATE_FEATURE_COUNT`] floats).
/// Output:
/// - eco logits (5) over the eco directions in [`EdgeCategory::ALL`].
/// - rush probability (1) for whether the economy is ready to start the goal.
///
/// The architecture is intentionally tiny: a shared two-layer backbone
/// ([`STATE_FEATURE_COUNT`] -> 128 -> 64) followed by two heads
/// (eco: 64 -> 5, rush: 64 -> 1).
///
/// The forward pass is split into methods so the trainer can reuse the backbone
/// output when computing both losses:
///
/// - `latent` uses `backbone1`, `backbone2`, and `activation`.
/// - `eco_logits` uses `eco_head`.
/// - `rush_logit` uses `rush_head`.
/// - [`evaluate`](Self::evaluate) is the public convenience wrapper.
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    /// First shared backbone layer: input features -> 128-dim hidden space.
    backbone1: Linear<B>,
    /// Second shared backbone layer: 128-dim hidden -> 64-dim latent vector.
    backbone2: Linear<B>,
    /// ReLU activation used after every backbone layer.
    activation: Relu,
    /// Eco head: latent (64) -> 5 logits over eco directions.
    eco_head: Linear<B>,
    /// Rush head: latent (64) -> 1 logit converted to a probability.
    rush_head: Linear<B>,
}

impl<B: Backend> HierarchicalPolicyNet<B> {
    /// Create a new direction-only policy network.
    ///
    /// Architecture:
    /// - backbone: STATE_FEATURE_COUNT -> 128 -> 64
    /// - eco head: 64 -> 5
    /// - rush head: 64 -> 1
    pub fn new(device: &B::Device) -> Self {
        let backbone_input = STATE_FEATURE_COUNT;
        let backbone_hidden = 128;
        let latent_dim = 64;

        Self {
            backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
            backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
            activation: Relu::new(),
            eco_head: LinearConfig::new(latent_dim, ECO_DIRECTION_COUNT).init(device),
            rush_head: LinearConfig::new(latent_dim, 1).init(device),
        }
    }

    /// Return a Graphviz DOT description of this network's architecture.
    pub fn to_dot(&self) -> String {
        hierarchical_policy_net_dot()
    }

    /// Shared backbone that turns a batch of state feature vectors into a batch
    /// of 64-dimensional latent vectors.
    ///
    /// `pub(crate)` because this is an internal building block. Callers outside
    /// `faf-sim` should use [`evaluate_direction`](Self::evaluate_direction), which
    /// wraps a single feature vector into a batch and runs both the backbone and
    /// the direction head. The training code calls `latent` + `direction_logits`
    /// separately so it can reuse the latent vector when computing multiple losses.
    ///
    /// `Tensor<B, 2>` means a two-dimensional tensor on backend `B`. The first
    /// dimension is the batch size and the second is the feature count. For
    /// inference that is usually `[1, STATE_FEATURE_COUNT]`; during training it is
    /// also `[1, STATE_FEATURE_COUNT]` because REINFORCE updates one step at a
    /// time, but the batch dimension keeps the API compatible with future
    /// mini-batch training.
    ///
    /// Input shape: `[batch_size, STATE_FEATURE_COUNT]`. Output shape: `[batch_size, 64]`.
    ///
    /// This is just a stack of linear layers separated by activations. "Backbone"
    /// means the feature-extraction part before the task-specific head. You could
    /// add a `backbone3` layer, change the hidden sizes, or swap `Relu` for
    /// `Gelu`, `Sigmoid`, etc. — the only requirement is that the final output
    /// shape matches whatever `direction_head` expects.
    ///
    /// Rule of thumb: if the struct has `n` backbone `Linear` layers, call
    /// `.forward()` on each of them once, in order, and apply an activation
    /// after each one. The current code has two backbone layers, so `latent()`
    /// contains two `Linear::forward` calls and two `activation.forward` calls.
    pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.backbone1.forward(features);
        let x = self.activation.forward(x);
        let x = self.backbone2.forward(x);
        self.activation.forward(x)
    }

    /// Eco logits from a latent vector.
    ///
    /// Input shape: `[batch_size, 64]`. Output shape: `[batch_size, 5]`.
    /// The five values correspond to the eco directions in
    /// [`ECO_DIRECTION_INDICES`](crate::planner::policy::macro_net::ECO_DIRECTION_INDICES).
    pub(crate) fn eco_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.eco_head.forward(latent)
    }

    /// Rush logit from a latent vector.
    ///
    /// Input shape: `[batch_size, 64]`. Output shape: `[batch_size, 1]`.
    pub(crate) fn rush_logit(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.rush_head.forward(latent)
    }

    /// Convenience: evaluate both heads on a single feature vector.
    ///
    /// Returns the raw eco logits and the rush probability.
    pub fn evaluate(&self, features: Vec<f32>, device: &B::Device) -> (Vec<f32>, f32) {
        let features = tensor_from_vec(&features, device);
        let latent = self.latent(features);

        let eco_logits = self.eco_logits(latent.clone());
        let rush_logit = self.rush_logit(latent);

        let eco = eco_logits.into_data().as_slice::<f32>().unwrap().to_vec();
        let rush = rush_logit.into_data().as_slice::<f32>().unwrap()[0];
        let rush_p = sigmoid(rush);

        (eco, rush_p)
    }

    /// Backward-compatible convenience that returns only the eco logits.
    pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        self.evaluate(features, device).0
    }
}

/// Compatibility alias for code that refers to the old `PolicyBundle`.
pub type PolicyBundle<B> = HierarchicalPolicyNet<B>;

/// Scalar sigmoid for converting a rush logit to a probability.
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Build a 2-D tensor of shape `[1, features.len()]` from a feature vector.
fn tensor_from_vec<B: Backend>(features: &[f32], device: &B::Device) -> Tensor<B, 2> {
    let data = TensorData::new(features.to_vec(), [1, features.len()]);
    Tensor::<B, 2>::from_data(data, device)
}

/// Apply a boolean mask to logits: illegal positions become `MASK_VALUE`.
pub fn apply_mask(logits: &mut [f32], mask: &[bool]) {
    for (l, legal) in logits.iter_mut().zip(mask.iter()) {
        if !legal {
            *l = MASK_VALUE;
        }
    }
}

/// Deterministically select the highest-scoring legal item.
pub fn masked_argmax(logits: &[f32], mask: &[bool]) -> Option<usize> {
    logits
        .iter()
        .zip(mask.iter())
        .enumerate()
        .filter(|(_, (_, &legal))| legal)
        .max_by(|(_, (a, _)), (_, (b, _))| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}

/// Sample an index from a softmax over the legal logits.
pub fn masked_sample_index(logits: &[f32], mask: &[bool], rng: &mut ThreadRng) -> Option<usize> {
    let mut masked = logits.to_vec();
    apply_mask(&mut masked, mask);
    let probs = softmax_probs(&masked);
    let dist = WeightedIndex::new(&probs).ok()?;
    Some(dist.sample(rng))
}

/// Compute a probability vector from raw logits using numerically stable softmax.
pub fn softmax_probs(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&s| (s - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum <= 0.0 || !sum.is_finite() {
        return vec![1.0f32 / logits.len() as f32; logits.len()];
    }
    exps.iter().map(|&e| e / sum).collect()
}

/// Return a Graphviz DOT description of the direction-only policy network
/// architecture.
pub fn hierarchical_policy_net_dot() -> String {
    let input_dim = STATE_FEATURE_COUNT;
    let backbone_hidden = 128;
    let latent_dim = 64;

    format!(
        r##"digraph DirectionPolicyNet {{
    rankdir=LR;
    node [shape=box, style="rounded,filled", fontname="Helvetica"];
    edge [fontname="Helvetica", fontsize=10];

    input [label="Input\n{} state features", fillcolor="#e3f2fd"];

    backbone1 [label="backbone1\nLinear({}, {})", fillcolor="#fff3e0"];
    relu1 [label="ReLU", shape=ellipse, fillcolor="#f3e5f5"];
    backbone2 [label="backbone2\nLinear({}, {})", fillcolor="#fff3e0"];
    relu2 [label="ReLU", shape=ellipse, fillcolor="#f3e5f5"];
    latent [label="Latent vector\n{}", fillcolor="#e8f5e9"];

    input -> backbone1;
    backbone1 -> relu1;
    relu1 -> backbone2;
    backbone2 -> relu2;
    relu2 -> latent;

    eco_head [label="eco_head\nLinear({}, {})", fillcolor="#ffebee"];
    eco_out [label="Eco logits\n{} (IncreaseMass/\nIncreaseEnergy/IncreaseBP/\nIncreaseEnergyStorage/\nUpgradeTech)", fillcolor="#ffcdd2"];
    rush_head [label="rush_head\nLinear({}, {})", fillcolor="#ffebee"];
    rush_out [label="Rush logit\n1", fillcolor="#ffcdd2"];
    latent -> eco_head;
    eco_head -> eco_out;
    latent -> rush_head;
    rush_head -> rush_out;
}}"##,
        input_dim,
        input_dim,
        backbone_hidden,
        backbone_hidden,
        latent_dim,
        latent_dim,
        ECO_DIRECTION_COUNT,
        ECO_DIRECTION_COUNT,
        ECO_DIRECTION_COUNT,
        1,
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inference-only backend for tests. Prefer CPU when available so unit tests
    /// do not require a GPU, but fall back to CUDA/WGPU if CPU is disabled.
    #[cfg(feature = "cpu")]
    type TestBackend = burn::backend::NdArray;
    #[cfg(all(feature = "cuda", not(feature = "cpu")))]
    type TestBackend = burn::backend::Cuda;
    #[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
    type TestBackend = burn::backend::Wgpu;

    #[test]
    fn direction_net_runs_forward_pass() {
        let device = Default::default();
        let net: HierarchicalPolicyNet<TestBackend> = HierarchicalPolicyNet::new(&device);

        let features = vec![0.0f32; STATE_FEATURE_COUNT];
        let latent = net.latent(tensor_from_vec(&features, &device));

        let eco_logits = net.eco_logits(latent.clone());
        assert_eq!(
            eco_logits.into_data().as_slice::<f32>().unwrap().len(),
            ECO_DIRECTION_COUNT
        );

        let rush_logit = net.rush_logit(latent);
        assert_eq!(rush_logit.into_data().as_slice::<f32>().unwrap().len(), 1);
    }

    #[test]
    fn masked_argmax_respects_mask() {
        let logits = vec![1.0f32, 5.0, 2.0, 3.0];
        let mask = vec![true, false, true, false];
        assert_eq!(masked_argmax(&logits, &mask), Some(2));
    }
}
