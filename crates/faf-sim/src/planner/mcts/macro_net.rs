//! Learned hierarchical macro-edge + build-power + engineer-squad policy.
//!
//! Three small MLPs are trained jointly with REINFORCE:
//!
//! 1. `MacroNet` selects a concrete plan-graph edge from state features plus
//!    engineer-shortfall feedback.
//! 2. `BuildPowerNet` decides how much build power to allocate to the selected
//!    edge.
//! 3. `EngineerSquadNet` decides the desired [T1, T2, T3] engineer counts to
//!    realize that build power.
//!
//! At inference all three networks are deterministic: argmax over legal edges,
//! then round/clamp for build power and the engineer squad.

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use super::features::{SHORTFALL_FEATURE_COUNT, STATE_FEATURE_COUNT};
use super::selections::PlanEdgeIndex;
use crate::units::{UnitKind, Units};

/// Standard deviation used when sampling build power during training.
pub const DEFAULT_POWER_STD: f32 = 2.0;
/// Standard deviation used when sampling engineer counts during training.
pub const DEFAULT_SQUAD_STD: f32 = 0.5;

/// Mask value that makes a logit numerically irrelevant after softmax.
pub(crate) const MASK_VALUE: f32 = -1e9;

/// Macro-edge policy network.
///
/// Input: base state features + previous-tick engineer shortfall.
/// Output: logits over the edges of the plan graph.
#[derive(Module, Debug)]
pub struct MacroNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}

impl<B: Backend> MacroNet<B> {
    /// Create a new macro network that outputs one logit per plan-graph edge.
    pub fn new(device: &B::Device, num_edges: usize) -> Self {
        Self {
            linear1: LinearConfig::new(STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT, 128)
                .init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(128, 64).init(device),
            output: LinearConfig::new(64, num_edges).init(device),
        }
    }

    /// Expected size of the input feature vector.
    pub fn input_dim(&self) -> usize {
        let [d_input, _] = self.linear1.weight.shape().dims();
        d_input
    }

    /// Number of edges this network scores.
    pub fn output_dim(&self) -> usize {
        let [_, d_output] = self.output.weight.shape().dims();
        d_output
    }

    /// Evaluate a batch of feature vectors.
    pub fn forward(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.linear1.forward(features);
        let x = self.activation.forward(x);
        let x = self.linear2.forward(x);
        let x = self.activation.forward(x);
        self.output.forward(x)
    }

    /// Convenience wrapper for a single feature vector.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap().to_vec()
    }
}

/// Build-power network.
///
/// Input: base state features + one-hot selected edge.
/// Output: scalar target build power.
#[derive(Module, Debug)]
pub struct BuildPowerNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}

impl<B: Backend> BuildPowerNet<B> {
    /// Create a new build-power network.
    pub fn new(device: &B::Device, num_edges: usize) -> Self {
        Self {
            linear1: LinearConfig::new(STATE_FEATURE_COUNT + num_edges, 64).init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(64, 32).init(device),
            output: LinearConfig::new(32, 1).init(device),
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

    /// Convenience wrapper for a single feature vector.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Number of outputs from this network.
    pub fn output_dim(&self) -> usize {
        let [_, d_output] = self.output.weight.shape().dims();
        d_output
    }
}

/// Engineer-squad network.
///
/// Input: base state features + target build power.
/// Output: [T1, T2, T3] engineer counts.
#[derive(Module, Debug)]
pub struct EngineerSquadNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}

impl<B: Backend> EngineerSquadNet<B> {
    /// Create a new engineer-squad network.
    pub fn new(device: &B::Device) -> Self {
        Self {
            linear1: LinearConfig::new(STATE_FEATURE_COUNT + 1, 64).init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(64, 32).init(device),
            output: LinearConfig::new(32, 3).init(device),
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

    /// Convenience wrapper for a single feature vector.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Number of outputs from this network.
    pub fn output_dim(&self) -> usize {
        let [_, d_output] = self.output.weight.shape().dims();
        d_output
    }
}

/// Container for the three jointly-trained policy networks.
#[derive(Module, Debug)]
pub struct PolicyBundle<B: Backend> {
    /// Selects the concrete plan-graph edge to satisfy.
    pub macro_net: MacroNet<B>,
    /// Decides how much build power the selected edge needs.
    pub power_net: BuildPowerNet<B>,
    /// Decides the [T1, T2, T3] engineer counts for that build power.
    pub squad_net: EngineerSquadNet<B>,
}

impl<B: Backend> PolicyBundle<B> {
    /// Create a randomly-initialized policy for a plan graph with `num_edges`.
    pub fn new(device: &B::Device, num_edges: usize) -> Self {
        Self {
            macro_net: MacroNet::new(device, num_edges),
            power_net: BuildPowerNet::new(device, num_edges),
            squad_net: EngineerSquadNet::new(device),
        }
    }
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

/// Deterministically select the highest-scoring legal edge.
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
    let u1: f32 = rng.gen::<f32>().max(1e-7);
    let u2: f32 = rng.gen();
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

/// Infer the number of plan-graph edges for a goal unit.
pub fn num_plan_edges(units: &Units, goal: &UnitKind) -> Option<usize> {
    Some(units.plan_graph(goal).ok()?.graph().edge_count())
}

/// Build the stable edge index for a goal unit.
pub fn plan_edge_index(units: &Units, goal: &UnitKind) -> Option<PlanEdgeIndex> {
    let plan = units.plan_graph(goal).ok()?;
    Some(PlanEdgeIndex::new(&plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::PlannerConfig;
    use crate::planner::mcts::selections::{SelectionOption, SelectionPools};
    use crate::sim::GraphState;
    use crate::units::{TechLevel, UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn macro_net_runs_forward_pass() {
        let device = Default::default();
        let net: MacroNet<burn::backend::NdArray> = MacroNet::new(&device, 25);
        let features = vec![0.0f32; STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT];
        let scores = net.evaluate_single(features, &device);
        assert_eq!(scores.len(), 25);
        assert!(scores.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn build_power_net_runs_forward_pass() {
        let device = Default::default();
        let net: BuildPowerNet<burn::backend::NdArray> = BuildPowerNet::new(&device, 25);
        let features = vec![0.0f32; STATE_FEATURE_COUNT + 25];
        let out = net.evaluate_single(features, &device);
        assert_eq!(out.len(), 1);
        assert!(out[0].is_finite());
    }

    #[test]
    fn engineer_squad_net_runs_forward_pass() {
        let device = Default::default();
        let net: EngineerSquadNet<burn::backend::NdArray> = EngineerSquadNet::new(&device);
        let features = vec![0.0f32; STATE_FEATURE_COUNT + 1];
        let out = net.evaluate_single(features, &device);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn policy_bundle_dimensions_match_plan() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let num_edges = num_plan_edges(&units, &goal).expect("goal has a plan graph");
        let device = Default::default();
        let bundle: PolicyBundle<burn::backend::NdArray> = PolicyBundle::new(&device, num_edges);
        assert_eq!(bundle.macro_net.output_dim(), num_edges);
        assert_eq!(bundle.power_net.output_dim(), 1);
        assert_eq!(bundle.squad_net.output_dim(), 3);
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
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let plan = units.plan_graph(&goal).unwrap();
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
