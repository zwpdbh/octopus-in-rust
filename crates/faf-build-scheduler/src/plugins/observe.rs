//! Observation lifecycle step.
//!
//! Reads numeric economy and unit state from the Bevy world and turns it into
//! discrete symbolic conditions for the decision layer.

use bevy_ecs::prelude::*;

use faf_sim_shared::EcoSnapshot;

use crate::components::UnitKindComp;
use crate::resources::{EconomyState, SearchGoal};
use crate::search::SearchTarget;

/// Net energy margin below which the economy is considered stalled.
const STALLED_ENERGY_MARGIN: f64 = 0.0;
/// Net energy margin below which we should build power before anything else.
const THIN_ENERGY_MARGIN: f64 = 10.0;
/// Net energy margin considered comfortable.
const HEALTHY_ENERGY_MARGIN: f64 = 50.0;
/// Mass income threshold above which T2 tech upgrades become viable.
pub(crate) const TECH2_PRIORITY_MASS_THRESHOLD: f64 = 35.0;
/// Mass income threshold above which T3 tech upgrades become viable.
pub(crate) const TECH3_PRIORITY_MASS_THRESHOLD: f64 = 80.0;

/// Symbolic observation of the current scheduler state.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Observation {
    /// Current net energy situation.
    pub energy_margin: EnergyMargin,
    /// How current mass income compares to the eco target.
    pub mass_income_vs_target: MassIncomeVsTarget,
    /// How current mass production compares to the tech-upgrade thresholds.
    pub mass_production_tier: MassProductionTier,
    /// Whether the scheduler world owns a factory.
    pub has_factory: bool,
    /// Number of engineers owned.
    pub engineer_count: u32,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            energy_margin: EnergyMargin::Healthy,
            mass_income_vs_target: MassIncomeVsTarget::Below,
            mass_production_tier: MassProductionTier::BelowTech2,
            has_factory: false,
            engineer_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMargin {
    /// Net energy is strongly positive.
    Surplus,
    /// Net energy is positive and comfortable.
    Healthy,
    /// Net energy is positive but close to demand; build power soon.
    Thin,
    /// Net energy is negative; the economy is stalling.
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassIncomeVsTarget {
    /// Current mass income has not yet reached the target.
    Below,
    /// Current mass income satisfies the target within tolerance.
    Reached,
    /// Current mass income exceeds the target.
    Above,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassProductionTier {
    /// Mass production is below the T2 tech threshold.
    BelowTech2,
    /// Mass production is high enough to consider T2 tech.
    AtTech2,
    /// Mass production is high enough to consider T3 tech.
    AtTech3,
}

/// Observe the current scheduler world and write a symbolic [`Observation`].
pub(crate) fn observe_eco_system(
    mut observation: ResMut<Observation>,
    economy: Res<EconomyState>,
    goal: Res<SearchGoal>,
    units: Query<&UnitKindComp>,
) {
    *observation = observe(&economy.current, &goal.0, &units);
}

/// Build an observation from the current economy, goal, and unit entities.
pub(crate) fn observe(
    eco: &EcoSnapshot,
    goal: &SearchTarget,
    units: &Query<&UnitKindComp>,
) -> Observation {
    use faf_blueprints::UnitKind;

    let energy_demand = eco.maintenance_consumption_per_second_energy + eco.energy_drain;
    let net_energy = eco.production_per_second_energy - energy_demand;

    let energy_margin = if net_energy.value() <= STALLED_ENERGY_MARGIN {
        EnergyMargin::Stalled
    } else if net_energy.value() <= THIN_ENERGY_MARGIN {
        EnergyMargin::Thin
    } else if net_energy.value() <= HEALTHY_ENERGY_MARGIN {
        EnergyMargin::Healthy
    } else {
        EnergyMargin::Surplus
    };

    let mass_income_vs_target = if let SearchTarget::Eco(target) = goal {
        if target.is_reached(eco) {
            MassIncomeVsTarget::Reached
        } else if eco.production_per_second_mass.value() > target.mass_production.value() {
            MassIncomeVsTarget::Above
        } else {
            MassIncomeVsTarget::Below
        }
    } else {
        MassIncomeVsTarget::Below
    };

    let mass_production_tier = {
        let mass = eco.production_per_second_mass.value();
        if mass >= TECH3_PRIORITY_MASS_THRESHOLD {
            MassProductionTier::AtTech3
        } else if mass >= TECH2_PRIORITY_MASS_THRESHOLD {
            MassProductionTier::AtTech2
        } else {
            MassProductionTier::BelowTech2
        }
    };

    let has_factory = units.iter().any(|u| matches!(u.0, UnitKind::Factory(_)));
    let engineer_count = units
        .iter()
        .filter(|u| matches!(u.0, UnitKind::Engineer(_)))
        .count() as u32;

    Observation {
        energy_margin,
        mass_income_vs_target,
        mass_production_tier,
        has_factory,
        engineer_count,
    }
}
