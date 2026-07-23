//! Decide-direction lifecycle step.
//!
//! Computes per-direction confidence scores from the symbolic [`Observation`].

use bevy_ecs::prelude::*;

use faf_blueprints::TechLevel;
pub use faf_sim_shared::{DirectionScores, PriorityTable};

use super::observe::{
    EnergyMargin, EnergyStorageLevel, MassIncomeVsTarget, MassMargin, MassProductionTier,
    Observation,
};

/// Bevy resource wrapper for [`DirectionScores`].
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirectionScoresRes(pub DirectionScores);

/// Bevy resource wrapper for [`PriorityTable`].
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PriorityTableRes(pub PriorityTable);

/// Compute confidence scores from the current observation.
pub(crate) fn compute_direction_scores(obs: &Observation) -> DirectionScores {
    DirectionScores {
        energy: energy_score(obs),
        mass_income: mass_income_score(obs),
        build_power: build_power_score(obs),
        tech_t2: tech_score(obs, TechLevel::T2),
        tech_t3: tech_score(obs, TechLevel::T3),
    }
}

fn energy_score(obs: &Observation) -> u8 {
    match (obs.energy_margin, obs.energy_storage_level) {
        (EnergyMargin::MoreThanNeed, EnergyStorageLevel::Full) => 10,
        (EnergyMargin::JustEnough, EnergyStorageLevel::Full) => 20,
        (EnergyMargin::NeedMorePower, EnergyStorageLevel::Full) => 30,
        (EnergyMargin::MoreThanNeed, EnergyStorageLevel::NotFull) => 60,
        (EnergyMargin::JustEnough, EnergyStorageLevel::NotFull) => 80,
        (EnergyMargin::NeedMorePower, EnergyStorageLevel::NotFull) => 100,
    }
}

fn mass_income_score(obs: &Observation) -> u8 {
    match obs.mass_margin {
        MassMargin::Stall => 100,
        _ if obs.mass_income_vs_target == MassIncomeVsTarget::Below => 100,
        _ => 0,
    }
}

fn build_power_score(obs: &Observation) -> u8 {
    let idle = obs.idle_engineers.t1 + obs.idle_engineers.t2 + obs.idle_engineers.t3;
    if idle < 2 {
        30
    } else {
        0
    }
}

fn tech_score(obs: &Observation, desired: TechLevel) -> u8 {
    let tier_reached = match desired {
        TechLevel::T2 => obs.mass_production_tier == MassProductionTier::AtTech2,
        TechLevel::T3 => obs.mass_production_tier == MassProductionTier::AtTech3,
        _ => false,
    };
    if tier_reached && obs.mass_income_vs_target != MassIncomeVsTarget::Below {
        match desired {
            TechLevel::T3 => 100,
            TechLevel::T2 => 80,
            _ => 0,
        }
    } else {
        0
    }
}

/// Compute priority weights from the current observation.
///
/// * Mass is boosted when storage is low/`Stall` and reduced when it is high.
/// * Energy is boosted when the margin is thin/stalling and reduced when
///   strongly positive.
/// * Build power is boosted when mass is overflowing (so we spend mass on
///   engineers) and reduced when mass is scarce.
///
/// The priority is applied to the whole score, so the values are kept within a
/// range that cannot flip a higher-confidence direction behind a lower one.
pub(crate) fn compute_priority_table(obs: &Observation) -> PriorityTable {
    let mass = match obs.mass_margin {
        MassMargin::Stall => 10,
        MassMargin::Good => 7,
        MassMargin::Normal => 5,
        MassMargin::NeedToSpend => 3,
        MassMargin::Overflow => 1,
    };

    let energy = match (obs.energy_margin, obs.energy_storage_level) {
        (EnergyMargin::MoreThanNeed, EnergyStorageLevel::Full) => 1,
        (EnergyMargin::JustEnough, EnergyStorageLevel::Full) => 2,
        (EnergyMargin::NeedMorePower, EnergyStorageLevel::Full) => 3,
        (EnergyMargin::MoreThanNeed, EnergyStorageLevel::NotFull) => 6,
        (EnergyMargin::JustEnough, EnergyStorageLevel::NotFull) => 8,
        (EnergyMargin::NeedMorePower, EnergyStorageLevel::NotFull) => 10,
    };

    // Build-power priority is driven by mass storage: when mass is overflowing
    // we want to spend it on engineers; when mass is scarce we cannot afford
    // more build power.
    let build_power = match obs.mass_margin {
        MassMargin::Overflow => 10,
        MassMargin::NeedToSpend => 8,
        MassMargin::Normal => 5,
        MassMargin::Good => 3,
        MassMargin::Stall => 1,
    };

    PriorityTable {
        mass,
        energy,
        build_power,
    }
}

/// Compute per-direction confidence scores and priority weights from the
/// current observation and write them to the resource wrappers.
pub(crate) fn decide_eco_direction_system(
    observation: Res<Observation>,
    mut scores: ResMut<DirectionScoresRes>,
    mut priorities: ResMut<PriorityTableRes>,
) {
    scores.0 = compute_direction_scores(&observation);
    priorities.0 = compute_priority_table(&observation);
}
