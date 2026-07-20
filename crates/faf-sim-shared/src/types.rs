//! Shared, serializable input/output types for the FAF simulator and solver.
//!
//! These types are deliberately lightweight so that callers (the Dioxus web app,
//! the CLI, tests, the WebSocket service, and the analytical solver) can describe
//! units and queues without depending on the full ECS runtime.

use faf_blueprints::UnitEcoStats;
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
use serde::{Deserialize, Serialize};

/// One task in a build queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildTask {
    /// Caller-defined id, echoed back in start/complete events.
    pub id: u32,
    /// Delay after the previous task finishes before this task may begin.
    ///
    /// For the first task this is a delay relative to simulation start (time 0).
    #[serde(default = "default_start_after")]
    pub start_after: Time,

    /// Builders assigned to the task.
    pub builders: Vec<UnitEcoStats>,
    /// Units being built, in order. Builders work through the list sequentially.
    pub targets: Vec<UnitEcoStats>,
}

fn default_start_after() -> Time {
    Time::from_raw(1.0)
}

/// A full build queue to simulate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildQueue {
    /// Initial economy state (income and storage).
    pub initial_eco: EconomyRuntimeState,
    /// Tasks to run, in queue order.
    pub tasks: Vec<BuildTask>,
}

/// A point-in-time view of the economy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoSnapshot {
    pub time: f64,
    /// Gross FAF `ProductionPerSecondMass`.
    pub production_per_second_mass: f64,
    /// Gross FAF `ProductionPerSecondEnergy`.
    pub production_per_second_energy: f64,
    /// Total FAF `MaintenanceConsumptionPerSecondEnergy` paid by all owned units.
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: f64,
    /// Total mass requested by all active construction sites per second.
    #[serde(default)]
    pub mass_drain: f64,
    /// Total energy requested by all active construction sites per second.
    #[serde(default)]
    pub energy_drain: f64,
    pub total_mass_spent: f64,
    pub total_energy_spent: f64,
    /// Current mass stored.
    pub mass_storage: f64,
    /// Mass storage capacity.
    #[serde(default)]
    pub mass_storage_cap: f64,
    /// Current energy stored.
    pub energy_storage: f64,
    /// Energy storage capacity.
    #[serde(default)]
    pub energy_storage_cap: f64,
}

/// `EcoSnapshot` is a flat, primitive view of one tick and includes construction
/// drain rates; `EconomyRuntimeState` is the typed, evolving state that the
/// simulator mutates to produce those snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EconomyRuntimeState {
    /// Gross mass produced per second (FAF `ProductionPerSecondMass`).
    pub production_per_second_mass: MassRate,
    /// Gross energy produced per second (FAF `ProductionPerSecondEnergy`).
    /// Maintenance is tracked separately in [`maintenance_consumption_per_second_energy`]
    /// and subtracted each tick.
    pub production_per_second_energy: EnergyRate,
    /// Total FAF `MaintenanceConsumptionPerSecondEnergy` paid by all owned units.
    /// Used to compute the FAF-standard energy efficiency ratio that scales
    /// `ProductionPerSecondMass` during stalls.
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: EnergyRate,
    /// Mass storage (current amount + capacity).
    pub mass_storage: Storage<Mass>,
    /// Energy storage (current amount + capacity).
    pub energy_storage: Storage<Energy>,
}
