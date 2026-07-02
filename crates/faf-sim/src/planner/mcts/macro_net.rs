//! Learned hierarchical policy network.
//!
//! This network implements a single shared backbone with four task-specific
//! heads that make a build-order decision in stages. The design follows the
//! natural hierarchy of a FAF build order:
//!
//! 1. **What strategic focus?** — direction head (`IncreaseMass`,
//!    `IncreaseEnergy`, `IncreaseBP`, or `Goal`).
//! 2. **Which concrete plan-graph edge?** — action head scores every legal edge
//!    within the chosen focus.
//! 3. **How much build power?** — power head predicts the total build power to
//!    assign to the selected edge.
//! 4. **Which engineers?** — squad head predicts the desired `[T1, T2, T3]`
//!    engineer counts to deliver that power.
//!
//! Each later head conditions on the output of the previous head (the chosen
//! direction, edge, or power is concatenated to the shared latent vector). This
//! makes credit assignment easier: the direction head learns high-level
//! strategy, the action head learns which unit to build, and the lower heads
//! learn only the resource allocation for that choice.
//!
//! At inference the policy is deterministic: masked argmax over legal
//! directions, then masked argmax over the legal edges in that direction, then
//! round/clamp for build power and the engineer squad. During training the same
//! heads are sampled from softmax distributions, and the log-probabilities of
//! all four choices are combined for the REINFORCE update.

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
///
/// The directions are `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, and
/// `Goal`, in the order defined by [`EdgeCategory::ALL`].
pub const DIRECTION_COUNT: usize = 4;

/// Number of factory-upgrade options the upgrade head can choose from.
///
/// The three options are:
///
/// 1. Do not upgrade a factory this step.
/// 2. Upgrade `Factory(T1) -> Factory(T2)`.
/// 3. Upgrade `Factory(T2) -> Factory(T3)`.
pub const UPGRADE_OPTION_COUNT: usize = 3;

/// Standard deviation used when sampling build power during training.
pub const DEFAULT_POWER_STD: f32 = 2.0;
/// Standard deviation used when sampling engineer counts during training.
pub const DEFAULT_SQUAD_STD: f32 = 0.5;

/// Mask value that makes a logit numerically irrelevant after softmax.
pub(crate) const MASK_VALUE: f32 = -1e9;

/// Hierarchical policy network.
///
/// Input: base state features + previous-tick engineer shortfall (16 floats).
/// Outputs: direction logits (4), action logits over plan-graph edges
/// (`num_edges`), scalar target build power (1), and `[T1, T2, T3]` engineer
/// counts (3).
///
/// The architecture is intentionally small: a shared two-layer backbone
/// (16 -> 128 -> 64) followed by four shallow heads. Each head is just one
/// hidden layer and one output layer so that training stays stable and fast.
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    /// First shared backbone layer: input features -> 128-dim hidden space.
    backbone1: Linear<B>,
    /// Second shared backbone layer: 128-dim hidden -> 64-dim latent vector.
    backbone2: Linear<B>,
    /// ReLU activation used after every backbone and head hidden layer.
    activation: Relu,
    /// Direction head: latent (64) -> 4 logits over `EdgeCategory`.
    direction_head: Linear<B>,
    /// Upgrade head: latent (64) -> 3 logits over factory-upgrade options.
    upgrade_head: Linear<B>,
    /// Action head hidden layer: latent + one-hot direction (68) -> 128.
    action_hidden: Linear<B>,
    /// Action head output layer: 128 -> `num_edges` logits over plan-graph edges.
    action_head: Linear<B>,
    /// Power head hidden layer: latent + one-hot selected edge (64 + N) -> 64.
    power_hidden: Linear<B>,
    /// Power head output layer: 64 -> 1 scalar target build power.
    power_head: Linear<B>,
    /// Squad head hidden layer: latent + target power (65) -> 64.
    squad_hidden: Linear<B>,
    /// Squad head output layer: 64 -> 3 engineer counts [T1, T2, T3].
    squad_head: Linear<B>,
}

impl<B: Backend> HierarchicalPolicyNet<B> {
    /// Create a new hierarchical policy for a plan graph with `num_edges`.
    ///
    /// `num_edges` is the number of selectable plan-graph edges for the current
    /// goal; it determines the output dimension of `action_head`. All other
    /// dimensions are fixed:
    ///
    /// - backbone: 16 -> 128 -> 64
    /// - direction head: 64 -> 4
    /// - upgrade head: 64 -> 3
    /// - action head: (64 + 4) -> 128 -> `num_edges`
    /// - power head: (64 + `num_edges`) -> 64 -> 1
    /// - squad head: (64 + 1) -> 64 -> 3
    pub fn new(device: &B::Device, num_edges: usize) -> Self {
        let backbone_input = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
        let backbone_hidden = 128;
        let latent_dim = 64;

        Self {
            backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
            backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
            activation: Relu::new(),
            direction_head: LinearConfig::new(latent_dim, DIRECTION_COUNT).init(device),
            upgrade_head: LinearConfig::new(latent_dim, UPGRADE_OPTION_COUNT).init(device),
            action_hidden: LinearConfig::new(latent_dim + DIRECTION_COUNT, 128).init(device),
            action_head: LinearConfig::new(128, num_edges).init(device),
            power_hidden: LinearConfig::new(latent_dim + num_edges, 64).init(device),
            power_head: LinearConfig::new(64, 1).init(device),
            squad_hidden: LinearConfig::new(latent_dim + 1, 64).init(device),
            squad_head: LinearConfig::new(64, 3).init(device),
        }
    }

    /// Return a Graphviz DOT description of this network's architecture.
    ///
    /// `num_edges` is the number of plan-graph edges the action head scores.
    /// The output can be rendered with `dot -Tsvg` or Graphviz tools.
    pub fn to_dot(&self, num_edges: usize) -> String {
        hierarchical_policy_net_dot(num_edges)
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
    /// Input shape: `[batch_size, 64]`. Output shape: `[batch_size, 4]`.
    /// The four values correspond to `EdgeCategory::ALL` in order:
    /// `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `Goal`.
    pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.direction_head.forward(latent)
    }

    /// Factory-upgrade logits from a latent vector.
    ///
    /// Input shape: `[batch_size, 64]`.
    /// Output shape: `[batch_size, 3]`.
    /// The three values correspond to:
    ///   0: do not upgrade a factory,
    ///   1: upgrade `Factory(T1) -> Factory(T2)`,
    ///   2: upgrade `Factory(T2) -> Factory(T3)`.
    pub(crate) fn upgrade_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
        self.upgrade_head.forward(latent)
    }

    /// Action logits from a latent vector and a one-hot chosen direction.
    ///
    /// The chosen direction is concatenated to the latent vector so the action
    /// head conditions on the high-level focus selected by the direction head.
    /// Input shapes: `[batch_size, 64]` and `[batch_size, 4]`.
    /// Output shape: `[batch_size, num_edges]`.
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
    ///
    /// The selected edge is concatenated to the latent vector so the power head
    /// knows which unit/upgrade is being built.
    /// Input shapes: `[batch_size, 64]` and `[batch_size, num_edges]`.
    /// Output shape: `[batch_size, 1]`.
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
    ///
    /// The target power is concatenated to the latent vector so the squad head
    /// can decide how many engineers of each tier are needed to deliver it.
    /// Input shapes: `[batch_size, 64]` and `[batch_size, 1]`.
    /// Output shape: `[batch_size, 3]`.
    pub(crate) fn squad_means(&self, latent: Tensor<B, 2>, power: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = Tensor::cat(vec![latent, power], 1);
        let x = self.squad_hidden.forward(x);
        let x = self.activation.forward(x);
        self.squad_head.forward(x)
    }

    /// Convenience: evaluate the upgrade head on a single feature vector.
    pub fn evaluate_upgrade(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let tensor = tensor_from_vec(&features, device);
        let logits = self.upgrade_logits(self.latent(tensor));
        logits.into_data().as_slice::<f32>().unwrap().to_vec()
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

/// Return a Graphviz DOT description of the hierarchical policy network
/// architecture for a plan graph with `num_edges` edges.
///
/// This does not require a network instance or backend, so it can be used from
/// tooling that only knows the number of plan-graph edges.
pub fn hierarchical_policy_net_dot(num_edges: usize) -> String {
    let input_dim = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
    let backbone_hidden = 128;
    let latent_dim = 64;
    let action_hidden = 128;
    let power_hidden = 64;
    let squad_hidden = 64;

    format!(
        r##"digraph HierarchicalPolicyNet {{
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
    direction_out [label="Direction logits\n{} (IncreaseMass/\nIncreaseEnergy/IncreaseBP/Goal)", fillcolor="#ffcdd2"];
    latent -> direction_head;
    direction_head -> direction_out;

    upgrade_head [label="upgrade_head\nLinear({}, {})", fillcolor="#ffebee"];
    upgrade_out [label="Upgrade logits\n{} (NoUpgrade/\nT1Factory->T2/\nT2Factory->T3)", fillcolor="#ffcdd2"];
    latent -> upgrade_head;
    upgrade_head -> upgrade_out;

    concat_action [label="Concat\n{} + {}", shape=diamond, fillcolor="#e0f7fa"];
    action_hidden [label="action_hidden\nLinear({}, {})", fillcolor="#fff3e0"];
    relu_action [label="ReLU", shape=ellipse, fillcolor="#f3e5f5"];
    action_head [label="action_head\nLinear({}, {})", fillcolor="#fff3e0"];
    action_out [label="Action logits\n{} plan-graph edges", fillcolor="#ffcdd2"];

    latent -> concat_action;
    direction_out -> concat_action [style=dashed, constraint=false];
    concat_action -> action_hidden;
    action_hidden -> relu_action;
    relu_action -> action_head;
    action_head -> action_out;

    concat_power [label="Concat\n{} + {}", shape=diamond, fillcolor="#e0f7fa"];
    power_hidden [label="power_hidden\nLinear({}, {})", fillcolor="#fff3e0"];
    relu_power [label="ReLU", shape=ellipse, fillcolor="#f3e5f5"];
    power_head [label="power_head\nLinear({}, {})", fillcolor="#fff3e0"];
    power_out [label="Target build power\n1 scalar", fillcolor="#ffcdd2"];

    latent -> concat_power;
    action_out -> concat_power [style=dashed, constraint=false];
    concat_power -> power_hidden;
    power_hidden -> relu_power;
    relu_power -> power_head;
    power_head -> power_out;

    concat_squad [label="Concat\n{} + 1", shape=diamond, fillcolor="#e0f7fa"];
    squad_hidden [label="squad_hidden\nLinear({}, {})", fillcolor="#fff3e0"];
    relu_squad [label="ReLU", shape=ellipse, fillcolor="#f3e5f5"];
    squad_head [label="squad_head\nLinear({}, {})", fillcolor="#fff3e0"];
    squad_out [label="Engineer counts\n[T1, T2, T3]", fillcolor="#ffcdd2"];

    latent -> concat_squad;
    power_out -> concat_squad [style=dashed, constraint=false];
    concat_squad -> squad_hidden;
    squad_hidden -> relu_squad;
    relu_squad -> squad_head;
    squad_head -> squad_out;
}}"##,
        input_dim,
        STATE_FEATURE_COUNT,
        SHORTFALL_FEATURE_COUNT,
        input_dim,
        backbone_hidden,
        backbone_hidden,
        latent_dim,
        latent_dim,
        latent_dim,
        DIRECTION_COUNT,
        DIRECTION_COUNT,
        latent_dim,
        UPGRADE_OPTION_COUNT,
        UPGRADE_OPTION_COUNT,
        latent_dim,
        DIRECTION_COUNT,
        latent_dim + DIRECTION_COUNT,
        action_hidden,
        action_hidden,
        num_edges,
        num_edges,
        latent_dim,
        num_edges,
        latent_dim + num_edges,
        power_hidden,
        power_hidden,
        1,
        latent_dim,
        latent_dim,
        squad_hidden,
        squad_hidden,
        3,
    )
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
    use crate::sim::SimulationState;
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
        let state = SimulationState::new(&units, &[UnitKind::Commander]);
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
