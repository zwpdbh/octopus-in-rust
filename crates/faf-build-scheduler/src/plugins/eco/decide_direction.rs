//! Decide-direction lifecycle step.
//!
//! Computes per-direction confidence scores from the symbolic [`Observation`].
#![allow(unused)]
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
    todo!("not implemented")
}

/// Compute per-direction confidence scores and priority weights from the
/// current observation and write them to the resource wrappers.
pub(crate) fn decide_eco_direction_system(
    observation: Res<Observation>,
    mut scores: ResMut<DirectionScoresRes>,
    mut priorities: ResMut<PriorityTableRes>,
) {
    todo!("not implemented")
}
