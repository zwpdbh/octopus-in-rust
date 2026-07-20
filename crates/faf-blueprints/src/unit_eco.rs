//! Lightweight economic descriptor used by the simulator and blueprint library.
//!
//! These types live here so that `faf-blueprints` can build the blueprint
//! economic table without depending on the full `faf-sim` runtime.

use serde::{Deserialize, Serialize};

/// Adjacency bonuses applied to a unit after it is built.
///
/// The planner decides how a unit is placed relative to bonus-giving structures.
/// The runtime uses these counts to compute effective production and
/// consumption multipliers.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AdjacencyBonus {
    /// Number of fully-surrounded sides by adjacent mass storage.
    /// Each side increases mass production of a mass-producing receiver by
    /// [`Self::STORAGE_BONUS_PER_SIDE`].
    #[serde(default)]
    pub mass_storage_sides: u8,
    /// Number of fully-surrounded sides by adjacent energy storage.
    /// Each side increases energy production of an energy-producing receiver by
    /// [`Self::STORAGE_BONUS_PER_SIDE`].
    #[serde(default)]
    pub energy_storage_sides: u8,
}

impl AdjacencyBonus {
    /// Maximum number of sides that can be fully surrounded on a 1x1 building.
    pub const MAX_SIDES: u8 = 4;

    /// Bonus per fully-surrounded side for mass/energy storage adjacency.
    ///
    /// FAF gives a maximum of +50% production when a producer is fully
    /// surrounded by storage, so each of the four sides contributes 12.5%.
    pub const STORAGE_BONUS_PER_SIDE: f64 = 0.125;

    /// Multiplier applied to mass production from mass-storage adjacency.
    pub fn mass_production_multiplier(&self) -> f64 {
        let sides = self.mass_storage_sides.min(Self::MAX_SIDES) as f64;
        1.0 + sides * Self::STORAGE_BONUS_PER_SIDE
    }

    /// Multiplier applied to energy production from energy-storage adjacency.
    pub fn energy_production_multiplier(&self) -> f64 {
        let sides = self.energy_storage_sides.min(Self::MAX_SIDES) as f64;
        1.0 + sides * Self::STORAGE_BONUS_PER_SIDE
    }
}

/// Lightweight economic descriptor used by the simulator.
///
/// It deliberately does not depend on the full blueprint library, so callers
/// can describe units with whatever data they already have.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UnitEcoStats {
    /// Build power contributed while building. Zero for non-builders.
    #[serde(default)]
    pub build_power: f64,
    /// Mass required to build the unit.
    #[serde(default)]
    pub mass_cost: f64,
    /// Energy required to build the unit.
    #[serde(default)]
    pub energy_cost: f64,
    /// Build time at base build power (1.0).
    #[serde(default)]
    pub build_time: f64,
    /// FAF `ProductionPerSecondMass` produced by the unit after it finishes.
    #[serde(default)]
    pub production_per_second_mass: f64,
    /// FAF `ProductionPerSecondEnergy` produced by the unit after it finishes.
    #[serde(default)]
    pub production_per_second_energy: f64,
    /// FAF `MaintenanceConsumptionPerSecondEnergy` paid per second while the unit exists.
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: f64,
    /// Mass storage capacity provided after the unit is finished.
    #[serde(default)]
    pub mass_storage: f64,
    /// Energy storage capacity provided after the unit is finished.
    #[serde(default)]
    pub energy_storage: f64,
    /// Optional adjacency bonuses for this unit.
    #[serde(default)]
    pub adjacency: AdjacencyBonus,
    /// Optional unit identifier carried through for round-tripping from UI plans.
    #[serde(default)]
    pub unit_id: Option<String>,
}

impl UnitEcoStats {
    /// Drain per second for building this unit with the given power.
    pub fn drain_per_second(&self, power: f64) -> (f64, f64) {
        if self.build_time <= 0.0 || power <= 0.0 {
            return (0.0, 0.0);
        }
        let progress_per_second = power / self.build_time;
        (
            progress_per_second * self.mass_cost,
            progress_per_second * self.energy_cost,
        )
    }
}
