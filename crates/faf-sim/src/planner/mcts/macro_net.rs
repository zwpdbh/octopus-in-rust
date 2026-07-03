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

use super::features::{SHORTFALL_FEATURE_COUNT, STATE_FEATURE_COUNT};

/// Number of strategic directions the direction head can choose from.
///
/// The directions are `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`,
/// `IncreaseEnergyStorage`, `Goal`, and `UpgradeTech`, in the order defined by
/// [`EdgeCategory::ALL`].
pub const DIRECTION_COUNT: usize = 6;

/// Mask value that makes a logit numerically irrelevant after softmax.
pub(crate) const MASK_VALUE: f32 = -1e9;

/// Direction-only policy network.
///
/// Input: base state features + previous-tick engineer shortfall (16 floats).
/// Output: direction logits (6) over [`EdgeCategory::ALL`].
///
/// The architecture is intentionally tiny: a shared two-layer backbone
/// (16 -> 128 -> 64) followed by a single direction head (64 -> 6).
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    /// First shared backbone layer: input features -> 128-dim hidden space.
    backbone1: Linear<B>,
    /// Second shared backbone layer: 128-dim hidden -> 64-dim latent vector.
    backbone2: Linear<B>,
    /// ReLU activation used after every backbone layer.
    activation: Relu,
    /// Direction head: latent (64) -> 6 logits over `EdgeCategory`.
    direction_head: Linear<B>,
}

impl<B: Backend> HierarchicalPolicyNet<B> {
    /// Create a new direction-only policy network.
    ///
    /// Architecture:
    /// - backbone: 16 -> 128 -> 64
    /// - direction head: 64 -> 6
    pub fn new(device: &B::Device) -> Self {
        let backbone_input = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
        let backbone_hidden = 128;
        let latent_dim = 64;

        Self {
            backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
            backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
            activation: Relu::new(),
            direction_head: LinearConfig::new(latent_dim, DIRECTION_COUNT).init(device),
        }
    }

    /// Return a Graphviz DOT description of this network's architecture.
    pub fn to_dot(&self) -> String {
        hierarchical_policy_net_dot()
    }

    /// Shared backbone that turns a batch of state feature vectors into a batch
    /// of 64-dimensional latent vectors.
    ///
    /// Input shape: `[batch_size, 16]`. Output shape: `[batch_size, 64]`.
    pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.backbone1.forward(features);
        let x = self.activation.forward(x);
        let x = self.backbone2.forward(x);
        self.activation.forward(x)
    }

    /// Direction logits from a latent vector.
    ///
    /// Input shape: `[batch_size, 64]`. Output shape: `[batch_size, 6]`.
    /// The six values correspond to `EdgeCategory::ALL` in order:
    /// `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `IncreaseEnergyStorage`,
    /// `Goal`, `UpgradeTech`.
    pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.direction_head.forward(latent)
    }

    /// Convenience: evaluate the direction head on a single feature vector.
    pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let tensor = tensor_from_vec(&features, device);
        let logits = self.direction_logits(self.latent(tensor));
        logits.into_data().as_slice::<f32>().unwrap().to_vec()
    }
}

/// Compatibility alias for code that refers to the old `PolicyBundle`.
pub type PolicyBundle<B> = HierarchicalPolicyNet<B>;

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
    let input_dim = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
    let backbone_hidden = 128;
    let latent_dim = 64;

    format!(
        r##"digraph DirectionPolicyNet {{
    rankdir=LR;
    node [shape=box, style="rounded,filled", fontname="Helvetica"];
    edge [fontname="Helvetica", fontsize=10];

    input [label="Input\n{} features\n({} state + {} shortfall)", fillcolor="#e3f2fd"];

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

    direction_head [label="direction_head\nLinear({}, {})", fillcolor="#ffebee"];
    direction_out [label="Direction logits\n{} (IncreaseMass/\nIncreaseEnergy/IncreaseBP/\nIncreaseEnergyStorage/Goal/\nUpgradeTech)", fillcolor="#ffcdd2"];
    latent -> direction_head;
    direction_head -> direction_out;
}}"##,
        input_dim,
        STATE_FEATURE_COUNT,
        SHORTFALL_FEATURE_COUNT,
        input_dim,
        backbone_hidden,
        backbone_hidden,
        latent_dim,
        latent_dim,
        DIRECTION_COUNT,
        DIRECTION_COUNT,
        DIRECTION_COUNT,
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

        let features = vec![0.0f32; STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT];
        let latent = net.latent(tensor_from_vec(&features, &device));

        let direction = net.direction_logits(latent);
        assert_eq!(
            direction.into_data().as_slice::<f32>().unwrap().len(),
            DIRECTION_COUNT
        );
    }

    #[test]
    fn masked_argmax_respects_mask() {
        let logits = vec![1.0f32, 5.0, 2.0, 3.0];
        let mask = vec![true, false, true, false];
        assert_eq!(masked_argmax(&logits, &mask), Some(2));
    }
}
