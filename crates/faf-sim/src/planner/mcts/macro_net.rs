//! Learned macro-direction policy network.
//!
//! The network predicts a distribution over high-level build priorities from
//! economy/state features only. A deterministic resolver then turns the chosen
//! priority into a concrete [`SelectionOption`].

use burn::module::Module;
use burn::nn::{Linear, LinearConfig, Relu};
use burn::tensor::backend::Backend;
use burn::tensor::{Tensor, TensorData};
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use super::features::{state_features, STATE_FEATURE_COUNT};
use super::selections::SelectionOption;
use crate::planner::core::PlannerConfig;
use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};

/// Number of macro directions the policy chooses among.
pub const MACRO_DIRECTION_COUNT: usize = 4;

/// High-level build priority produced by the learned policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MacroDirection {
    /// Increase available build power: engineers, factories, or assisting.
    BuildPower = 0,
    /// Increase mass income: mexes, upgrades, capped mexes.
    MoreMass = 1,
    /// Increase energy income: pgens, energy storage.
    MorePower = 2,
    /// Unlock the next tech tier: factory upgrades or higher-tier factories.
    TechUp = 3,
}

impl MacroDirection {
    /// Convert a network output index back into a direction.
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(MacroDirection::BuildPower),
            1 => Some(MacroDirection::MoreMass),
            2 => Some(MacroDirection::MorePower),
            3 => Some(MacroDirection::TechUp),
            _ => None,
        }
    }

    /// Human-readable label for logging.
    pub fn label(self) -> &'static str {
        match self {
            MacroDirection::BuildPower => "BuildPower",
            MacroDirection::MoreMass => "MoreMass",
            MacroDirection::MorePower => "MorePower",
            MacroDirection::TechUp => "TechUp",
        }
    }
}

/// Classify a concrete candidate into its macro direction.
///
/// `Assist` is always classified as `BuildPower` because it multiplies existing
/// build power; it does not generate mass/power income or unlock tech.
pub fn macro_direction_of(option: &SelectionOption, _state: &GraphState) -> MacroDirection {
    match option {
        SelectionOption::Assist(_) => MacroDirection::BuildPower,
        SelectionOption::Build(target) | SelectionOption::Upgrade { to: target, .. } => {
            macro_direction_of_kind(target)
        }
    }
}

fn macro_direction_of_kind(kind: &UnitKind) -> MacroDirection {
    match kind {
        UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex => MacroDirection::MoreMass,
        UnitKind::Pgen(_) | UnitKind::EnergyStorage => MacroDirection::MorePower,
        UnitKind::Factory(t) if *t >= TechLevel::T2 => MacroDirection::TechUp,
        UnitKind::Engineer(_) | UnitKind::Factory(_) | UnitKind::Commander => {
            MacroDirection::BuildPower
        }
        UnitKind::Unique(_) => MacroDirection::BuildPower,
    }
}

/// Small MLP that maps state features to a score for each macro direction.
///
/// Input shape: `[batch, STATE_FEATURE_COUNT]`.
/// Output shape: `[batch, MACRO_DIRECTION_COUNT]`.
#[derive(Module, Debug)]
pub struct MacroNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}

impl<B: Backend> MacroNet<B> {
    /// Create a new macro policy network.
    ///
    /// Architecture: STATE_FEATURE_COUNT -> 128 -> 64 -> MACRO_DIRECTION_COUNT.
    pub fn new(device: &B::Device) -> Self {
        Self {
            linear1: LinearConfig::new(STATE_FEATURE_COUNT, 128).init(device),
            activation: Relu::new(),
            linear2: LinearConfig::new(128, 64).init(device),
            output: LinearConfig::new(64, MACRO_DIRECTION_COUNT).init(device),
        }
    }

    /// Expected size of the input feature vector.
    pub fn input_dim(&self) -> usize {
        let [d_input, _] = self.linear1.weight.shape().dims();
        d_input
    }

    /// Evaluate a batch of state feature vectors.
    pub fn forward(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.linear1.forward(features);
        let x = self.activation.forward(x);
        let x = self.linear2.forward(x);
        let x = self.activation.forward(x);
        self.output.forward(x)
    }

    /// Convenience wrapper for a single state feature vector.
    ///
    /// Returns a score for each macro direction.
    pub fn evaluate_single(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
        let feature_count = features.len();
        let data = TensorData::new(features, [1, feature_count]);
        let tensor = Tensor::from_data(data, device);
        let output = self.forward(tensor);
        output.into_data().as_slice::<f32>().unwrap().to_vec()
    }

    /// Score the four macro directions for the current state.
    pub fn score_directions(
        &self,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
        device: &B::Device,
    ) -> [f32; MACRO_DIRECTION_COUNT] {
        let features = state_features(state, units, config);
        let scores = self.evaluate_single(features, device);
        let mut result = [0.0f32; MACRO_DIRECTION_COUNT];
        for (i, &s) in scores.iter().enumerate().take(MACRO_DIRECTION_COUNT) {
            result[i] = s;
        }
        result
    }
}

/// Sample a macro direction from the softmax over network scores.
pub fn sample_direction(scores: &[f32], rng: &mut ThreadRng) -> MacroDirection {
    let n = scores.len().max(MACRO_DIRECTION_COUNT);
    let probs = softmax_probs(scores);
    let dist =
        WeightedIndex::new(&probs).unwrap_or_else(|_| WeightedIndex::new(vec![1.0f32; n]).unwrap());
    MacroDirection::from_index(dist.sample(rng)).unwrap_or(MacroDirection::BuildPower)
}

/// Deterministically pick the highest-scoring macro direction.
pub fn argmax_direction(scores: &[f32]) -> MacroDirection {
    scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|(i, _)| MacroDirection::from_index(i))
        .unwrap_or(MacroDirection::BuildPower)
}

/// Compute a probability vector from raw logits using numerically stable softmax.
pub(crate) fn softmax_probs(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return vec![1.0f32 / MACRO_DIRECTION_COUNT as f32; MACRO_DIRECTION_COUNT];
    }
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&s| (s - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum <= 0.0 || !sum.is_finite() {
        return vec![1.0f32 / logits.len() as f32; logits.len()];
    }
    exps.iter().map(|&e| e / sum).collect()
}

/// Resolve a macro direction into a concrete, executable candidate option.
///
/// If the chosen direction has no available candidates, returns `None` so the
/// caller can fall back to the next-best direction.
pub fn resolve_macro_direction(
    direction: MacroDirection,
    candidates: &[SelectionOption],
    state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
    config: &PlannerConfig,
) -> Option<SelectionOption> {
    let filtered: Vec<_> = candidates
        .iter()
        .filter(|c| macro_direction_of(c, state) == direction)
        .cloned()
        .collect();

    if filtered.is_empty() {
        return None;
    }

    match direction {
        MacroDirection::BuildPower => resolve_build_power(&filtered, state, units, plan),
        MacroDirection::MoreMass => resolve_more_mass(&filtered, state, units, plan),
        MacroDirection::MorePower => resolve_more_power(&filtered, state, units, plan, config),
        MacroDirection::TechUp => resolve_tech_up(&filtered, state, units, plan),
    }
}

/// Resolve `BuildPower` by preferring assists, then the best build-power unit.
fn resolve_build_power(
    candidates: &[SelectionOption],
    state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
) -> Option<SelectionOption> {
    let (assists, builds): (Vec<_>, Vec<_>) = candidates
        .iter()
        .partition(|c| matches!(c, SelectionOption::Assist(_)));

    if !assists.is_empty() {
        // Assist the active project with the highest-value target.
        assists
            .into_iter()
            .max_by(|a, b| {
                let va = assist_value(a, state, units);
                let vb = assist_value(b, state, units);
                va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    } else {
        // Build/upgrade the unit with the highest build-rate return per mass.
        builds
            .into_iter()
            .max_by(|a, b| {
                let sa = build_power_score(a, state, units, plan);
                let sb = build_power_score(b, state, units, plan);
                compare_resolver_scores(sa, sb)
            })
            .cloned()
    }
}

/// Resolve `MoreMass` by highest incremental mass income per mass cost.
fn resolve_more_mass(
    candidates: &[SelectionOption],
    state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
) -> Option<SelectionOption> {
    candidates
        .iter()
        .max_by(|a, b| {
            let sa = income_score(a, state, units, plan, /* mass */ true);
            let sb = income_score(b, state, units, plan, /* mass */ true);
            compare_resolver_scores(sa, sb)
        })
        .cloned()
}

/// Resolve `MorePower` by highest incremental energy income per mass cost.
fn resolve_more_power(
    candidates: &[SelectionOption],
    state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
    _config: &PlannerConfig,
) -> Option<SelectionOption> {
    candidates
        .iter()
        .max_by(|a, b| {
            let sa = income_score(a, state, units, plan, /* mass */ false);
            let sb = income_score(b, state, units, plan, /* mass */ false);
            compare_resolver_scores(sa, sb)
        })
        .cloned()
}

/// Resolve `TechUp` by the cheapest factory build/upgrade that unlocks a tier.
fn resolve_tech_up(
    candidates: &[SelectionOption],
    _state: &GraphState,
    units: &Units,
    _plan: &PlanGraph,
) -> Option<SelectionOption> {
    candidates
        .iter()
        .min_by(|a, b| {
            let ca = total_cost(a, units);
            let cb = total_cost(b, units);
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

/// Score used by `resolve_build_power`.
///
/// Tuple ordering: higher benefit/cost ratio first, then higher tier, then
/// shorter distance to goal.
fn build_power_score(
    option: &SelectionOption,
    _state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
) -> (f64, i8, usize) {
    let (target, effective_cost) = match option {
        SelectionOption::Build(t) => (t, mass_cost(t, units)),
        SelectionOption::Upgrade { from, to } => {
            // Upgrades cost less than a fresh build; use incremental cost.
            let incremental = (mass_cost(to, units) - mass_cost(from, units)).max(1.0);
            (to, incremental)
        }
        SelectionOption::Assist(_) => unreachable!("assists handled separately"),
    };

    let def = units.def(target).expect("unit must exist in index");
    let ratio = def.build_rate / effective_cost;
    let tier = tier_rank(target);
    let distance = crate::planner::mcts::features::distance_to_goal(plan, target);

    // Negate distance because shorter is better; ratio and tier are "higher is better".
    (ratio, tier, distance)
}

/// Score used by `resolve_more_mass` / `resolve_more_power`.
///
/// Tuple ordering: higher incremental income per mass cost first, then higher
/// tier, then shorter distance to goal.
fn income_score(
    option: &SelectionOption,
    _state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
    mass: bool,
) -> (f64, i8, usize) {
    let (target, from_kind, cost) = match option {
        SelectionOption::Build(t) => (t, None, mass_cost(t, units)),
        SelectionOption::Upgrade { from, to } => (to, Some(from), mass_cost(to, units)),
        SelectionOption::Assist(_) => unreachable!("assists handled separately"),
    };

    let def = units.def(target).expect("unit must exist in index");
    let new_income = if mass {
        def.mass_income
    } else {
        def.energy_income
    };

    let old_income = from_kind
        .and_then(|k| units.def(k))
        .map(|d| if mass { d.mass_income } else { d.energy_income })
        .unwrap_or(0.0);

    let incremental = (new_income - old_income).max(0.0);
    let ratio = incremental / cost.max(1.0);
    let tier = tier_rank(target);
    let distance = crate::planner::mcts::features::distance_to_goal(plan, target);

    (ratio, tier, distance)
}

/// Value of assisting an active project: prefer expensive, goal-relevant targets.
fn assist_value(option: &SelectionOption, state: &GraphState, units: &Units) -> f64 {
    if let SelectionOption::Assist(node) = option {
        let target = &state.graph[*node].unit_id;
        let cost = mass_cost(target, units);
        // A simple proxy for project importance: mass cost of the unit being built.
        cost
    } else {
        0.0
    }
}

/// Total resource cost of an option, with energy converted to mass-equivalent.
fn total_cost(option: &SelectionOption, units: &Units) -> f64 {
    match option {
        SelectionOption::Build(t) => units
            .build_cost(t)
            .map(|c| c.mass + c.energy / 10.0)
            .unwrap_or(f64::MAX),
        SelectionOption::Upgrade { from, to } => {
            let from_cost = units
                .build_cost(from)
                .map(|c| c.mass + c.energy / 10.0)
                .unwrap_or(0.0);
            let to_cost = units
                .build_cost(to)
                .map(|c| c.mass + c.energy / 10.0)
                .unwrap_or(f64::MAX);
            (to_cost - from_cost).max(1.0)
        }
        SelectionOption::Assist(_) => 0.0,
    }
}

fn mass_cost(kind: &UnitKind, units: &Units) -> f64 {
    units.build_cost(kind).map(|c| c.mass).unwrap_or(f64::MAX)
}

fn tier_rank(kind: &UnitKind) -> i8 {
    use crate::units::TechLevel;
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => {
            match t {
                TechLevel::T1 => 1,
                TechLevel::T2 => 2,
                TechLevel::T3 => 3,
                TechLevel::T4 => 4,
            }
        }
        UnitKind::CapT2Mex => 2,
        UnitKind::CapT3Mex => 3,
        UnitKind::EnergyStorage => 1,
        UnitKind::Commander => 0,
        UnitKind::Unique(_) => 5,
    }
}

/// Compare resolver score tuples.
///
/// All three tuple components are "higher is better" because distance is stored
/// as a raw `usize` but treated as higher-is-worse; the callers pass distance
/// directly, so this helper inverts the distance comparison.
fn compare_resolver_scores(a: (f64, i8, usize), b: (f64, i8, usize)) -> std::cmp::Ordering {
    // ratio higher is better, tier higher is better, distance lower is better
    a.0.partial_cmp(&b.0)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.1.cmp(&b.1).reverse())
        .then_with(|| a.2.cmp(&b.2).reverse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn macro_net_runs_forward_pass() {
        let device = Default::default();
        let net: MacroNet<burn::backend::NdArray> = MacroNet::new(&device);
        let features = vec![0.0f32; STATE_FEATURE_COUNT];
        let scores = net.evaluate_single(features, &device);
        assert_eq!(scores.len(), MACRO_DIRECTION_COUNT);
        assert!(scores.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn direction_classification() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let _plan = units.plan_graph(&goal).unwrap();
        let state = GraphState::new(&units, &[UnitKind::Commander]);

        assert_eq!(
            macro_direction_of(
                &SelectionOption::Build(UnitKind::Mex(TechLevel::T1)),
                &state
            ),
            MacroDirection::MoreMass
        );
        assert_eq!(
            macro_direction_of(
                &SelectionOption::Build(UnitKind::Pgen(TechLevel::T1)),
                &state
            ),
            MacroDirection::MorePower
        );
        assert_eq!(
            macro_direction_of(
                &SelectionOption::Build(UnitKind::Engineer(TechLevel::T1)),
                &state
            ),
            MacroDirection::BuildPower
        );
        assert_eq!(
            macro_direction_of(
                &SelectionOption::Build(UnitKind::Factory(TechLevel::T2)),
                &state
            ),
            MacroDirection::TechUp
        );
    }
}
