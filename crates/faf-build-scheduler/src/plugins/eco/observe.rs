//! Observation lifecycle step.
//!
//! Reads numeric economy and unit state from the Bevy world and turns it into
//! discrete symbolic conditions for the decision layer.

use bevy_ecs::prelude::*;

use faf_quantities::{EnergyRate, MassRate};
use faf_sim_shared::GameEcoMetrics;

use crate::components::{BuilderState, UnitKindComp};
use crate::resources::{GameEco, SearchGoal};
use crate::search::SearchTarget;

/// Symbolic observation of the current scheduler state.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Observation {
    eco: GameEcoMetrics,
    energy_drain: EnergyRate,
    mass_drain: MassRate,
}

impl Default for Observation {
    fn default() -> Self {
        Self {
            eco: GameEcoMetrics::default(),
            energy_drain: EnergyRate::zero(),
            mass_drain: MassRate::zero(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMargin {
    MoreThanNeed,
    JustEnough,
    NeedMorePower,
}

/// Energy storage buffer level, independent of net income.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyStorageLevel {
    NotFull,
    Full,
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

#[allow(unused)]
/// Observe the current scheduler world and write a symbolic [`Observation`].
pub(crate) fn observe_eco_system(
    mut observation: ResMut<Observation>,
    game_eco: Res<GameEco>,
    goal: Res<SearchGoal>,
    units: Query<(&UnitKindComp, &BuilderState)>,
) {
    let target = match &goal.0 {
        SearchTarget::Eco(target) => target,
        _ => todo!("unit observation is not implemented yet"),
    };
    todo!("not implemented: from units state to derive energy drain")
}
