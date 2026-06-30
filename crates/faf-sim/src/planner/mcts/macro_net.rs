//! Learned hierarchical policy network.
//!
//! A single network with a shared backbone and four heads:
//!
//! 1. **Direction head** — picks a strategic focus (`Mass`, `Energy`, `BuildPower`,
//!    or `Progress`).
//! 2. **Action head** — scores the legal plan-graph edges inside the chosen focus.
//! 3. **Power head** — decides how much build power to allocate to the selected
//!    edge.
//! 4. **Squad head** — decides the desired `[T1, T2, T3]` engineer counts to
//!    deliver that build power.
//!
//! At inference the network is deterministic: masked argmax over legal
//! directions, then masked argmax over the legal edges in that direction, then
//! round/clamp for build power and the engineer squad.

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};
use rand::distr::weighted::WeightedIndex;
use rand::distr::Distribution;
use rand::rngs::ThreadRng;
use rand::RngExt;

use super::features::{SHORTFALL_FEATURE_COUNT, STATE_FEATURE_COUNT};
use super::selections::PlanEdgeIndex;
use crate::planner::core::Goal;
use crate::planner::plan_graph::EdgeCategory;
use crate::units::Units;

/// Number of strategic directions the direction head can choose from.
pub const DIRECTION_COUNT: usize = 4;

/// Standard deviation used when sampling build power during training.
pub const DEFAULT_POWER_STD: f32 = 2.0;
/// Standard deviation used when sampling engineer counts during training.
pub const DEFAULT_SQUAD_STD: f32 = 0.5;

/// Mask value that makes a logit numerically irrelevant after softmax.
pub(crate) const MASK_VALUE: f32 = -1e9;

/// Hierarchical policy network.
///
/// Input: base state features + previous-tick engineer shortfall.
/// Outputs: direction logits, action logits over plan-graph edges, scalar target
/// build power, and `[T1, T2, T3]` engineer counts.
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    direction_head: Linear<B>,
    action_hidden: Linear<B>,
    action_head: Linear<B>,
    power_hidden: Linear<B>,
    power_head: Linear<B>,
    squad_hidden: Linear<B>,
    squad_head: Linear<B>,
}

impl<B: Backend> HierarchicalPolicyNet<B> {
    /// Create a new hierarchical policy for a plan graph with `num_edges`.
    pub fn new(device: &B::Device, num_edges: usize) -> Self {
        let backbone_input = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
        let backbone_hidden = 128;
        let latent_dim = 64;

        Self {
            backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
            backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
            activation: Relu::new(),
            direction_head: LinearConfig::new(latent_dim, DIRECTION_COUNT).init(device),
            action_hidden: LinearConfig::new(latent_dim + DIRECTION_COUNT, 128).init(device),
            action_head: LinearConfig::new(128, num_edges).init(device),
            power_hidden: LinearConfig::new(latent_dim + num_edges, 64).init(device),
            power_head: LinearConfig::new(64, 1).init(device),
            squad_hidden: LinearConfig::new(latent_dim + 1, 64).init(device),
            squad_head: LinearConfig::new(64, 3).init(device),
        }
    }

    /// Shared backbone that turns state features into a latent vector.
    pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.backbone1.forward(features);
        let x = self.activation.forward(x);
        let x = self.backbone2.forward(x);
        self.activation.forward(x)
    }

    /// Direction logits from a latent vector.
    pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.direction_head.forward(latent)
    }

    /// Action logits from a latent vector and a one-hot direction.
    pub(crate) fn action_logits(
        &self,
        latent: Tensor<B, 2>,
        direction_one_hot: Tensor<B, 2>,
    ) -> Tensor<B, 2> {
        let x = Tensor::cat(vec![latent, direction_one_hot], 1);
        let x = self.action_hidden.forward(x);
        let x = self.activation.forward(x);
        self.action_head.forward(x)
    }

    /// Scalar target build power from a latent vector and a one-hot selected edge.
    pub(crate) fn power_mean(
        &self,
        latent: Tensor<B, 2>,
        edge_one_hot: Tensor<B, 2>,
    ) -> Tensor<B, 2> {
        let x = Tensor::cat(vec![latent, edge_one_hot], 1);
        let x = self.power_hidden.forward(x);
        let x = self.activation.forward(x);
        self.power_head.forward(x)
    }

    /// `[T1, T2, T3]` engineer counts from a latent vector and target power.
    pub(crate) fn squad_means(&self, latent: Tensor<B, 2>, power: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = Tensor::cat(vec![latent, power], 1);
        let x = self.squad_hidden.forward(x);
        let x = self.activation.forward(x);
        self.squad_head.forward(x)
    }

    /// Convenience: evaluate the direction head on a single feature vector.
    pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let tensor = tensor_from_vec(&features, device);
        let logits = self.direction_logits(self.latent(tensor));
        logits.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Convenience: evaluate the action head on a single feature vector and
    /// chosen direction.
    pub fn evaluate_action(
        &self,
        features: Vec<f32>,
        direction: EdgeCategory,
        device: &B::Device,
    ) -> Vec<f32> {
        let latent = self.latent(tensor_from_vec(&features, device));
        let direction_one_hot =
            tensor_from_vec(&one_hot(direction as usize, DIRECTION_COUNT), device);
        let logits = self.action_logits(latent, direction_one_hot);
        logits.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Convenience: evaluate the power head on a single feature vector and
    /// selected edge.
    pub fn evaluate_power(
        &self,
        features: Vec<f32>,
        edge_idx: usize,
        num_edges: usize,
        device: &B::Device,
    ) -> f32 {
        let latent = self.latent(tensor_from_vec(&features, device));
        let edge_one_hot = tensor_from_vec(&one_hot(edge_idx, num_edges), device);
        let out = self.power_mean(latent, edge_one_hot);
        out.into_data().as_slice::<f32>().unwrap()[0]
    }

    /// Convenience: evaluate the squad head on a single feature vector and
    /// target power.
    pub fn evaluate_squad(&self, features: Vec<f32>, power: f32, device: &B::Device) -> Vec<f32> {
        let latent = self.latent(tensor_from_vec(&features, device));
        let power_tensor = tensor_from_vec(&vec![power], device);
        let out = self.squad_means(latent, power_tensor);
        out.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Number of plan-graph edges this network scores.
    pub fn num_edges(&self) -> usize {
        let [_, d_output] = self.action_head.weight.shape().dims();
        d_output
    }
}

/// Compatibility alias for code that refers to the old `PolicyBundle`.
pub type PolicyBundle<B> = HierarchicalPolicyNet<B>;

/// Build a 2-D tensor of shape `[1, features.len()]` from a feature vector.
fn tensor_from_vec<B: Backend>(features: &[f32], device: &B::Device) -> Tensor<B, 2> {
    let data = TensorData::new(features.to_vec(), [1, features.len()]);
    Tensor::<B, 2>::from_data(data, device)
}

/// Build a one-hot vector with `1.0` at `idx`.
pub fn one_hot(idx: usize, len: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; len];
    if idx < len {
        v[idx] = 1.0;
    }
    v
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

/// Sample a scalar from a Gaussian using the Box-Muller transform.
pub fn sample_gaussian(mean: f32, std: f32, rng: &mut ThreadRng) -> f32 {
    let u1: f32 = rng.random::<f32>().max(1e-7);
    let u2: f32 = rng.random();
    let r = (-2.0 * u1.ln()).sqrt();
    let theta = 2.0 * std::f32::consts::PI * u2;
    mean + std * r * theta.cos()
}

/// Round and clamp a continuous engineer-count vector to non-negative integers,
/// capping each tech level by the number of idle engineers available.
pub fn clamp_squad(raw: [f32; 3], available: [usize; 3]) -> [usize; 3] {
    let mut clamped = [0usize; 3];
    for i in 0..3 {
        let v = raw[i].round().max(0.0) as usize;
        clamped[i] = v.min(available[i]);
    }
    clamped
}

/// Ensure at least one engineer is requested when an edge is legal but the
/// network asked for zero of every tech.
pub fn ensure_minimum_squad(desired: [usize; 3], available: [usize; 3]) -> [usize; 3] {
    let total: usize = desired.iter().sum();
    if total > 0 {
        return desired;
    }
    let mut adjusted = desired;
    for i in (0..3).rev() {
        if available[i] > 0 {
            adjusted[i] = 1;
            break;
        }
    }
    adjusted
}

/// Compute shortfall feedback from desired and available counts.
pub fn shortfall_from_counts(desired: [usize; 3], available: [usize; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let diff = desired[i].saturating_sub(available[i]) as f32;
        out[i] = diff;
    }
    out
}

/// Infer the number of plan-graph edges for a goal.
pub fn num_plan_edges(units: &Units, goal: &Goal) -> Option<usize> {
    Some(units.plan_graph(*goal).graph().edge_count())
}

/// Build the stable edge index for a goal.
pub fn plan_edge_index(units: &Units, goal: &Goal) -> Option<PlanEdgeIndex> {
    let plan = units.plan_graph(*goal);
    Some(PlanEdgeIndex::new(&plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::PlannerConfig;
    use crate::planner::mcts::selections::{SelectionOption, SelectionPools};
    use crate::sim::GraphState;
    use crate::units::{TechLevel, UnitKind, Units};

    /// Inference-only backend for tests. Prefer CPU when available so unit tests
    /// do not require a GPU, but fall back to CUDA/WGPU if CPU is disabled.
    #[cfg(feature = "cpu")]
    type TestBackend = burn::backend::NdArray;
    #[cfg(all(feature = "cuda", not(feature = "cpu")))]
    type TestBackend = burn::backend::Cuda;
    #[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
    type TestBackend = burn::backend::Wgpu;

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn hierarchical_net_runs_forward_pass() {
        let device = Default::default();
        let net: HierarchicalPolicyNet<TestBackend> = HierarchicalPolicyNet::new(&device, 25);

        let features = vec![0.0f32; STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT];
        let latent = net.latent(tensor_from_vec(&features, &device));

        let direction = net.direction_logits(latent.clone());
        assert_eq!(
            direction.into_data().as_slice::<f32>().unwrap().len(),
            DIRECTION_COUNT
        );

        let direction_one_hot = tensor_from_vec(&one_hot(0, DIRECTION_COUNT), &device);
        let action = net.action_logits(latent.clone(), direction_one_hot);
        assert_eq!(action.into_data().as_slice::<f32>().unwrap().len(), 25);

        let edge_one_hot = tensor_from_vec(&one_hot(0, 25), &device);
        let power = net.power_mean(latent.clone(), edge_one_hot);
        assert_eq!(power.into_data().as_slice::<f32>().unwrap().len(), 1);

        let power_tensor = tensor_from_vec(&vec![5.0], &device);
        let squad = net.squad_means(latent, power_tensor);
        assert_eq!(squad.into_data().as_slice::<f32>().unwrap().len(), 3);
    }

    #[test]
    fn policy_bundle_dimensions_match_plan() {
        let units = load_units();
        let goal = Goal {
            tech_level: crate::units::TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        };
        let num_edges = num_plan_edges(&units, &goal).expect("goal has a plan graph");
        let device = Default::default();
        let bundle: PolicyBundle<TestBackend> = PolicyBundle::new(&device, num_edges);
        assert_eq!(bundle.num_edges(), num_edges);
    }

    #[test]
    fn masked_argmax_respects_mask() {
        let logits = vec![1.0f32, 5.0, 2.0, 3.0];
        let mask = vec![true, false, true, false];
        assert_eq!(masked_argmax(&logits, &mask), Some(2));
    }

    #[test]
    fn clamp_squad_caps_by_available() {
        let raw = [1.4f32, 2.6, -0.1];
        let available = [1, 2, 0];
        assert_eq!(clamp_squad(raw, available), [1, 2, 0]);
    }

    #[test]
    fn edge_index_maps_to_selection_option() {
        let units = load_units();
        let goal = Goal {
            tech_level: crate::units::TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        };
        let plan = units.plan_graph(goal);
        let edge_index = PlanEdgeIndex::new(&plan);
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();

        let mut found_build = false;
        for i in 0..edge_index.len() {
            if let Some(option) = edge_index.to_selection_option(i, &state, &units, &config) {
                if matches!(
                    option,
                    SelectionOption::Build(UnitKind::Factory(TechLevel::T1))
                ) {
                    found_build = true;
                }
            }
        }
        assert!(found_build, "ACU should be able to build a T1 factory");

        let pools = SelectionPools::new(&plan, &state, &units, &config);
        for option in pools.options() {
            let idx = edge_index
                .find_edge_for_option(option, &state, &units, &config)
                .expect("every pool option should map to an edge");
            let back = edge_index
                .to_selection_option(idx, &state, &units, &config)
                .expect("mapped edge should be legal");
            assert_eq!(*option, back);
        }
    }
}
