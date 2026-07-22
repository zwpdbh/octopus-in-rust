//! Observation lifecycle step.
//!
//! Reads numeric economy and unit state from the Bevy world and turns it into
//! discrete symbolic conditions for the decision layer.

use bevy_ecs::prelude::*;

use faf_sim_shared::EcoSnapshot;

use crate::components::{BuilderState, UnitKindComp};
use crate::request::EcoTarget;
use crate::resources::{EconomyState, SearchGoal};
use crate::search::SearchTarget;

/// Time-to-empty threshold (seconds) below which an energy deficit is considered
/// thin/imminent.
const THIN_SECONDS_THRESHOLD: f64 = 5.0;
/// Production-to-demand ratio above which the economy is considered strongly
/// positive.
const SURPLUS_PRODUCTION_RATIO: f64 = 1.2;
/// Mass storage ratio below which mass is considered comfortably low.
const MASS_GOOD_THRESHOLD: f64 = 0.20;
/// Mass storage ratio above which the scheduler should prioritize spending mass.
const MASS_NEED_TO_SPEND_THRESHOLD: f64 = 0.70;
/// Mass income threshold above which T2 tech upgrades become viable.
pub(crate) const TECH2_PRIORITY_MASS_THRESHOLD: f64 = 35.0;
/// Mass income threshold above which T3 tech upgrades become viable.
pub(crate) const TECH3_PRIORITY_MASS_THRESHOLD: f64 = 80.0;

/// Compute the energy margin for a snapshot without needing unit state.
pub(crate) fn compute_energy_margin(eco: &EcoSnapshot) -> EnergyMargin {
    let energy_demand = eco.maintenance_consumption_per_second_energy + eco.energy_drain;
    let net_energy = eco.production_per_second_energy - energy_demand;

    if eco.energy_storage.value() <= 0.0 && net_energy.value() < 0.0 {
        EnergyMargin::Stalled
    } else if net_energy.value() < 0.0 {
        let seconds_to_empty = eco.energy_storage.value() / -net_energy.value();
        if seconds_to_empty <= THIN_SECONDS_THRESHOLD {
            EnergyMargin::Thin
        } else {
            EnergyMargin::Unhealthy
        }
    } else {
        let production_ratio = if energy_demand.value() > 0.0 {
            eco.production_per_second_energy.value() / energy_demand.value()
        } else {
            f64::INFINITY
        };
        if production_ratio > SURPLUS_PRODUCTION_RATIO {
            EnergyMargin::Surplus
        } else {
            EnergyMargin::Healthy
        }
    }
}

/// Compute the mass margin for a snapshot without needing unit state.
pub(crate) fn compute_mass_margin(eco: &EcoSnapshot) -> MassMargin {
    let net_mass = eco.production_per_second_mass - eco.mass_drain;
    let mass_storage_ratio = if eco.mass_storage_cap.value() > 0.0 {
        eco.mass_storage.value() / eco.mass_storage_cap.value()
    } else {
        0.0
    };

    if eco.mass_storage.value() <= 0.0 && net_mass.value() < 0.0 {
        MassMargin::Stall
    } else if mass_storage_ratio >= 1.0 && net_mass.value() > 0.0 {
        MassMargin::Overflow
    } else if mass_storage_ratio > MASS_NEED_TO_SPEND_THRESHOLD {
        MassMargin::NeedToSpend
    } else if mass_storage_ratio < MASS_GOOD_THRESHOLD {
        MassMargin::Good
    } else {
        MassMargin::Normal
    }
}

/// Symbolic observation of the current scheduler state.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Observation {
    /// Net energy health, used by the eco rule engine to decide when to prioritize
    /// building power generation (e.g. when stalled, thin, or unhealthy).
    pub energy_margin: EnergyMargin,
    /// Mass storage health, intended for future rules that decide when to spend
    /// mass aggressively (`NeedToSpend` / `Overflow`) or conserve it (`Stall`).
    pub mass_margin: MassMargin,
    /// How close current mass income is to the eco search target; drives the
    /// decision to keep expanding mass extractors or switch to other priorities.
    pub mass_income_vs_target: MassIncomeVsTarget,
    /// Current mass production level relative to T2/T3 tech thresholds; used to
    /// decide when tech upgrades become viable.
    pub mass_production_tier: MassProductionTier,
    /// Highest factory tech tier owned by the scheduler world; used by future
    /// rules to gate factory construction, upgrades, and unit production.
    pub factory_tier: FactoryTier,
    /// Number of idle engineers per tech tier; used to estimate available build
    /// power and decide when to train more engineers.
    pub idle_engineers: EngineerCounts,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            energy_margin: EnergyMargin::Healthy,
            mass_margin: MassMargin::Normal,
            mass_income_vs_target: MassIncomeVsTarget::Below,
            mass_production_tier: MassProductionTier::BelowTech2,
            factory_tier: FactoryTier::None,
            idle_engineers: EngineerCounts::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMargin {
    /// Production exceeds demand by more than the healthy threshold.
    Surplus,
    /// Production exceeds demand, but only by a comfortable amount.
    Healthy,
    /// Production is below demand and storage will empty within seconds.
    Thin,
    /// Production is below demand; storage is buffered but draining.
    Unhealthy,
    /// Storage is empty/negative and production cannot meet demand.
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassMargin {
    /// Mass storage is full and production exceeds drain.
    Overflow,
    /// Mass storage is high; spend mass to avoid waste.
    NeedToSpend,
    /// Mass storage is in a comfortable middle range.
    Normal,
    /// Mass storage is low.
    Good,
    /// Mass storage is empty and production cannot meet drain.
    Stall,
}

/// Idle engineer counts grouped by tech tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineerCounts {
    /// Idle T1 engineers.
    pub t1: u32,
    /// Idle T2 engineers.
    pub t2: u32,
    /// Idle T3 engineers (T4 engineers, if any, are folded here).
    pub t3: u32,
}

/// Owned factory tech tier, extracted from the world’s unit entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryTier {
    /// No factory is owned.
    None,
    /// A T1 factory is owned.
    T1,
    /// A T2 factory is owned.
    T2,
    /// A T3 (or higher) factory is owned.
    T3,
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
    units: Query<(&UnitKindComp, &BuilderState)>,
) {
    let target = match &goal.0 {
        SearchTarget::Eco(target) => target,
        _ => todo!("unit observation is not implemented yet"),
    };
    *observation = observe(&economy.current, target, &units);
}

/// Build an observation from the current economy, eco goal, and unit entities.
fn observe(
    eco: &EcoSnapshot,
    goal: &EcoTarget,
    units: &Query<(&UnitKindComp, &BuilderState)>,
) -> Observation {
    use faf_blueprints::{TechLevel, UnitKind};

    let energy_margin = compute_energy_margin(eco);
    let mass_margin = compute_mass_margin(eco);

    let mass_income_vs_target = if goal.is_reached(eco) {
        MassIncomeVsTarget::Reached
    } else if eco.production_per_second_mass.value() > goal.mass_production.value() {
        MassIncomeVsTarget::Above
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

    let factory_tier = units
        .iter()
        .filter_map(|(kind, _state)| match &kind.0 {
            UnitKind::Factory(tech) => Some(*tech),
            _ => None,
        })
        .max()
        .map(|tech| match tech {
            TechLevel::T1 => FactoryTier::T1,
            TechLevel::T2 => FactoryTier::T2,
            TechLevel::T3 | TechLevel::T4 => FactoryTier::T3,
        })
        .unwrap_or(FactoryTier::None);

    let mut idle_engineers = EngineerCounts::default();
    for (kind, state) in units.iter() {
        if !matches!(state, BuilderState::Idle) {
            continue;
        }
        if let UnitKind::Engineer(tech) = &kind.0 {
            match tech {
                TechLevel::T1 => idle_engineers.t1 += 1,
                TechLevel::T2 => idle_engineers.t2 += 1,
                TechLevel::T3 | TechLevel::T4 => idle_engineers.t3 += 1,
            }
        }
    }

    Observation {
        energy_margin,
        mass_margin,
        mass_income_vs_target,
        mass_production_tier,
        factory_tier,
        idle_engineers,
    }
}
