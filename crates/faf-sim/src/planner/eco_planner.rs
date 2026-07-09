//! Eco planner: choose actions that increase mass income.
//!
//! Given a [`SimulationState`], the eco planner's only objective is to grow the
//! economy as fast as possible, measured by mass income per second.  The default
//! target is [`EcoPlanner::DEFAULT_TARGET_MASS_INCOME`] (1000 mass/s); once the
//! current state reaches or exceeds that income the planner returns
//! [`SimAction::Wait`].
//!
//! The planner can be driven by a learned value net (any type implementing
//! [`ValueNet`]) or by a simple built-in heuristic.  This keeps it usable both
//! during training and as a standalone baseline.

use crate::engine::simulation_state::SimulationState;
use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError};
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
use crate::planner::policy::direction_planner::{execute_action, plan_result_with_action};
use crate::planner::policy::features::state_features;
use crate::planner::policy::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::policy::macro_net::{
    masked_argmax, masked_sample_index, ECO_DIRECTION_INDICES,
};
use crate::planner::policy::value_net::{EcoValueNet, ValueNet};
use crate::planner::SimAction;
use crate::units::Units;

/// Default mass-income target used by [`EcoPlanner`].
pub const DEFAULT_TARGET_MASS_INCOME: f64 = 1000.0;

/// Planner whose sole objective is to increase mass income.
///
/// The planner is stateless apart from its configuration and target.  It does
/// not remember previous plans; callers should re-plan each tick in a reactive
/// loop.
#[derive(Debug, Clone)]
pub struct EcoPlanner {
    config: PlannerConfig,
    target_mass_income: f64,
}

impl EcoPlanner {
    /// Create an eco planner with the default mass-income target.
    pub fn new(config: PlannerConfig) -> Self {
        Self::with_target(config, DEFAULT_TARGET_MASS_INCOME)
    }

    /// Create an eco planner with a custom mass-income target.
    pub fn with_target(config: PlannerConfig, target_mass_income: f64) -> Self {
        Self {
            config,
            target_mass_income: target_mass_income.max(0.0),
        }
    }

    /// Mass-income target that signals "done growing".
    pub fn target_mass_income(&self) -> f64 {
        self.target_mass_income
    }

    /// Shared planner configuration.
    pub fn config(&self) -> &PlannerConfig {
        &self.config
    }

    /// Produce the next eco-oriented action from `state`.
    ///
    /// If the state's mass income already meets the target, the planner returns
    /// [`SimAction::Wait`].  Otherwise it selects one of the five eco directions
    /// ([`EdgeCategory::IncreaseMass`], [`EdgeCategory::IncreaseEnergy`],
    /// [`EdgeCategory::IncreaseBP`], [`EdgeCategory::IncreaseEnergyStorage`],
    /// [`EdgeCategory::UpgradeTech`]) using `policy` when provided, or a simple
    /// heuristic otherwise.
    ///
    /// The returned [`PlanResult`] follows the same reactive convention as
    /// [`Planner::plan`](crate::planner::Planner::plan): execute only
    /// `first_action`, advance the simulator, and call `plan` again.
    pub fn plan(
        &self,
        state: SimulationState,
        units: &Units,
        policy: Option<&dyn ValueNet>,
        deterministic: bool,
    ) -> Result<PlanResult, PlannerError> {
        if state.economy.net_mass_income.value() >= self.target_mass_income {
            let mut done = state.clone();
            done.tick(units, self.config.dt);
            return Ok(plan_result_with_action(done, SimAction::Wait));
        }

        let plan = build_plan_graph(units, Goal::default());
        let direction_mask = eco_direction_mask(&state, units, &self.config, &plan);

        if direction_mask.iter().all(|&b| !b) {
            let mut wait_state = state.clone();
            wait_state.tick(units, self.config.dt);
            return Ok(plan_result_with_action(wait_state, SimAction::Wait));
        }

        let direction = if let Some(bundle) = policy {
            select_network_direction(
                &state,
                units,
                &self.config,
                bundle,
                &direction_mask,
                deterministic,
            )
        } else {
            select_heuristic_direction(&direction_mask)
        };

        let action = direction_to_action(
            direction,
            &state,
            units,
            &self.config,
            &Goal::default(),
            &plan,
        );

        let mut new_state = state.clone();
        if execute_action(&mut new_state, &action, units, self.config.dt).is_err() {
            let mut fallback = state;
            fallback.tick(units, self.config.dt);
            return Ok(plan_result_with_action(fallback, SimAction::Wait));
        }

        Ok(plan_result_with_action(new_state, action))
    }

    /// Convenience: plan with a fresh, randomly-initialized eco value net.
    ///
    /// Useful for tests and for callers that want a network-shaped policy
    /// without having loaded a checkpoint.
    pub fn plan_with_default_net(
        &self,
        state: SimulationState,
        units: &Units,
        deterministic: bool,
    ) -> Result<PlanResult, PlannerError> {
        let default_net = EcoValueNet::new();
        self.plan(state, units, Some(&default_net), deterministic)
    }
}

impl Default for EcoPlanner {
    fn default() -> Self {
        Self::new(PlannerConfig::default())
    }
}

/// Build a boolean mask over the five eco directions.
fn eco_direction_mask(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    plan: &PlanGraph,
) -> Vec<bool> {
    ECO_DIRECTION_INDICES
        .iter()
        .map(|&i| {
            let direction = EdgeCategory::ALL[i];
            is_direction_legal(direction, state, units, config, &Goal::default(), plan)
        })
        .collect()
}

/// Use the value net to pick the best legal eco direction.
fn select_network_direction(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    bundle: &dyn ValueNet,
    eco_mask: &[bool],
    deterministic: bool,
) -> EdgeCategory {
    let features = state_features(state, units, config);
    let eco_logits = bundle.evaluate_direction(features);

    let best_eco_idx = if deterministic {
        masked_argmax(&eco_logits, eco_mask)
    } else {
        let mut rng = rand::rng();
        masked_sample_index(&eco_logits, eco_mask, &mut rng)
    }
    .unwrap_or(0);

    EdgeCategory::ALL[ECO_DIRECTION_INDICES[best_eco_idx]]
}

/// Simple heuristic fallback when no value net is supplied.
///
/// Prefers mass, then build power, then energy, then storage, then tech
/// upgrades.  This is a conservative baseline: mass income is the win
/// condition, build power multiplies all future construction, and the remaining
/// directions cover prerequisites.
fn select_heuristic_direction(eco_mask: &[bool]) -> EdgeCategory {
    // `eco_mask` is ordered like [`ECO_DIRECTION_INDICES`].
    let ordered = [
        EdgeCategory::IncreaseMass,
        EdgeCategory::IncreaseEnergy,
        EdgeCategory::IncreaseBP,
        EdgeCategory::IncreaseEnergyStorage,
        EdgeCategory::UpgradeTech,
    ];

    let preference: std::collections::HashMap<EdgeCategory, usize> = [
        (EdgeCategory::IncreaseMass, 0),
        (EdgeCategory::IncreaseBP, 1),
        (EdgeCategory::IncreaseEnergy, 2),
        (EdgeCategory::IncreaseEnergyStorage, 3),
        (EdgeCategory::UpgradeTech, 4),
    ]
    .into_iter()
    .collect();

    ordered
        .iter()
        .enumerate()
        .filter(|(i, _)| eco_mask.get(*i).copied().unwrap_or(false))
        .map(|(_, &d)| d)
        .min_by_key(|d| preference.get(d).copied().unwrap_or(usize::MAX))
        .unwrap_or(EdgeCategory::IncreaseMass)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn eco_planner_returns_wait_when_target_met() {
        let units = load_units();
        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        state.economy.net_mass_income =
            crate::quantities::MassRate::from_raw(DEFAULT_TARGET_MASS_INCOME + 1.0);

        let planner = EcoPlanner::default();
        let result = planner
            .plan(state, &units, None, true)
            .expect("plan should succeed");

        assert_eq!(result.first_action, Some(SimAction::Wait));
    }

    #[test]
    fn eco_planner_selects_mass_from_acu_with_heuristic() {
        let units = load_units();
        let state = SimulationState::new(&units, &[UnitKind::Commander]);

        let planner = EcoPlanner::default();
        let result = planner
            .plan(state, &units, None, true)
            .expect("plan should succeed");

        assert!(
            matches!(
                result.first_action,
                Some(SimAction::Build { ref unit_id, .. })
                    if *unit_id == UnitKind::Mex(TechLevel::T1)
            ),
            "heuristic should build a T1 mex from the ACU, got {:?}",
            result.first_action
        );
    }
}
