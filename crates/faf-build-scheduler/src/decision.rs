//! Rule-based decision layer for the scheduler.
//!
//! Decisions are made by matching a symbolic [`Observation`] against an ordered
//! list of rules. This keeps the "what should we do?" logic separate from the
//! numeric observation code and from the action-efficiency heuristics.

use bevy_ecs::prelude::*;

use faf_blueprints::TechLevel;

use crate::observation::{EnergyMargin, MassIncomeVsTarget, MassProductionTier, Observation};

/// The currently chosen economic direction.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentEcoDirection(pub EcoDirection);

impl Default for CurrentEcoDirection {
    fn default() -> Self {
        Self(EcoDirection::MassIncome)
    }
}

/// Economic direction the scheduler should emphasize for the current step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoDirection {
    /// Advance tech by upgrading to the given tier.
    Tech(TechLevel),
    /// Increase mass income as efficiently as possible.
    MassIncome,
    /// Increase available build power.
    BuildPower,
    /// Increase energy income to avoid stalls.
    Energy,
}

/// A single condition → consequence rule.
pub(crate) struct Rule<Consequence> {
    #[allow(dead_code)]
    pub name: &'static str,
    pub condition: fn(&Observation) -> bool,
    pub consequence: Consequence,
}

/// The default eco rule engine.
pub(crate) fn eco_rules() -> Vec<Rule<EcoDirection>> {
    vec![
        Rule {
            name: "prevent energy stall",
            condition: |obs| {
                matches!(
                    obs.energy_margin,
                    EnergyMargin::Thin | EnergyMargin::Stalled
                )
            },
            consequence: EcoDirection::Energy,
        },
        Rule {
            name: "tech to T3 when mass is high",
            condition: |obs| obs.mass_production_tier == MassProductionTier::AtTech3,
            consequence: EcoDirection::Tech(TechLevel::T3),
        },
        Rule {
            name: "tech to T2 when mass is moderate",
            condition: |obs| obs.mass_production_tier == MassProductionTier::AtTech2,
            consequence: EcoDirection::Tech(TechLevel::T2),
        },
        Rule {
            name: "increase mass income until target reached",
            condition: |obs| obs.mass_income_vs_target == MassIncomeVsTarget::Below,
            consequence: EcoDirection::MassIncome,
        },
        Rule {
            name: "default to build power",
            condition: |_| true,
            consequence: EcoDirection::BuildPower,
        },
    ]
}

/// Apply the first matching rule from `rules` to `observation`.
pub(crate) fn decide<Consequence: Clone>(
    rules: &[Rule<Consequence>],
    observation: &Observation,
) -> Option<Consequence> {
    rules
        .iter()
        .find(|rule| (rule.condition)(observation))
        .map(|rule| rule.consequence.clone())
}

/// Decide the eco direction from the current observation and write it to the
/// [`CurrentEcoDirection`] resource.
pub(crate) fn decide_eco_direction_system(
    observation: Res<Observation>,
    mut direction: ResMut<CurrentEcoDirection>,
) {
    let rules = eco_rules();
    if let Some(chosen) = decide(&rules, &observation) {
        direction.0 = chosen;
    }
}
