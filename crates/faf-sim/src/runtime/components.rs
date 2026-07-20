//! Bevy ECS components used by the economy runtime.

use bevy_ecs::prelude::*;

use faf_blueprints::UnitEcoStats;

/// A unit that contributes FAF `ProductionPerSecondMass` / `ProductionPerSecondEnergy`
/// and/or pays FAF `MaintenanceConsumptionPerSecondEnergy`.
///
/// Every existing unit has this component, even builders with zero production.
/// This unifies the economic view of the world so the economy system only needs
/// a single query.
#[derive(Component)]
pub(crate) struct Producer {
    /// FAF `ProductionPerSecondMass`.
    pub(crate) production_per_second_mass: f64,
    /// Gross FAF `ProductionPerSecondEnergy` produced by this unit.
    pub(crate) production_per_second_energy: f64,
    /// FAF `MaintenanceConsumptionPerSecondEnergy` paid per second while the unit exists.
    pub(crate) maintenance_consumption_per_second_energy: f64,
}

/// Adjacency bonuses that modify this unit's effective production.
///
/// This is the ECS component form of [`faf_blueprints::AdjacencyBonus`].
/// It is attached to every entity that has a [`Producer`]; the runtime applies
/// the derived multipliers when recomputing base economy.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct AdjacencyBonusComp(pub faf_blueprints::AdjacencyBonus);

/// A unit that contributes mass/energy storage capacity.
#[derive(Component)]
pub(crate) struct StorageContributor {
    pub(crate) mass: f64,
    pub(crate) energy: f64,
}

/// An active construction site. Build power is applied each tick until the
/// current target finishes, then the site advances to the next target.
#[derive(Component)]
pub(crate) struct ActiveBuildTask {
    pub(crate) task_id: u32,
    pub(crate) targets: Vec<UnitEcoStats>,
    pub(crate) current_target_index: usize,
    pub(crate) remaining_work: f64,
    pub(crate) power: f64,
}
