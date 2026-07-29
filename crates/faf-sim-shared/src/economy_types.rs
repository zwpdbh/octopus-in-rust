//! Shared, serializable input/output types for the FAF simulator and solver.
//!
//! These types are deliberately lightweight so that callers (the Dioxus web app,
//! the CLI, tests, the WebSocket service, and the analytical solver) can describe
//! units and queues without depending on the full ECS runtime.

use std::f32::consts::E;

use faf_blueprints::{UnitCost, UnitEcoStats};
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
    pub initial_eco: GameEcoMetrics,
    /// Tasks to run, in queue order.
    pub tasks: Vec<BuildTask>,
}

/// `EcoSnapshot` is a flat, primitive view of one tick and includes construction
/// drain rates; `EconomyRuntimeState` is the typed, evolving state that the
/// simulator mutates to produce those snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GameEcoMetrics {
    pub production_per_second_mass: f64,
    pub production_energy_per_second: f64,
    pub maintenance_consumption_per_second_energy: f64,
    /// Mass storage (current amount + capacity).
    pub mass_storage_current: f64,
    pub mass_storage_capacity: f64,

    pub energy_storage_current: f64,
    pub energy_storage_capacity: f64,
}

impl Default for GameEcoMetrics {
    fn default() -> Self {
        Self {
            production_per_second_mass: 1.0,
            production_energy_per_second: 20.0,
            maintenance_consumption_per_second_energy: 0.0,
            mass_storage_current: 650.0,
            mass_storage_capacity: 650.0,
            energy_storage_current: 4000.0,
            energy_storage_capacity: 4000.0,
        }
    }
}

impl GameEcoMetrics {
    /// FAF army-wide energy efficiency ratio used to scale mass income.
    pub fn energy_efficiency(&self, energy_drain: f64) -> f64 {
        let requested = self.maintenance_consumption_per_second_energy + energy_drain;
        if requested <= 0.0 {
            1.0
        } else {
            (self.production_energy_per_second / requested).min(1.0)
        }
    }

    pub fn mass_efficiency(&self, energy_drain: f64) -> f64 {
        let net_energy_income = self.production_energy_per_second
            - self.maintenance_consumption_per_second_energy
            - energy_drain;
        if self.energy_storage_current + net_energy_income < 0.0 {
            self.energy_efficiency(energy_drain)
        } else {
            1.0
        }
    }
}

/// A EcoSnapshot is GameEcoParameters + Time
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoSnapshot {
    pub time: f64,
    pub eco_metrics: GameEcoMetrics,
    pub mass_drain_per_second: f64,
    pub energy_drain_per_second: f64,
}

pub const EPS: f64 = 1e-9;

impl EcoSnapshot {
    pub fn init_with_eco_parameters(game_eco_metrics: &GameEcoMetrics) -> Self {
        Self {
            time: 0.0,
            eco_metrics: *game_eco_metrics,
            mass_drain_per_second: 0.0,
            energy_drain_per_second: 0.0,
        }
    }

    /// Mass income scaled by energy efficiency when energy is depleted.
    ///
    /// `extra_drain` is the energy consumed by construction this tick; it is
    /// `0.0` for idle ticks.
    fn player_net_mass_income(&self) -> f64 {
        let actual_mass_production_per_second = self.eco_metrics.production_per_second_mass
            * self
                .eco_metrics
                .mass_efficiency(self.energy_drain_per_second);
        actual_mass_production_per_second - self.mass_drain_per_second
    }

    fn player_net_energy_income(&self) -> f64 {
        self.eco_metrics.production_energy_per_second
            - self.eco_metrics.maintenance_consumption_per_second_energy
            - self.energy_drain_per_second
    }

    /// Advance one second with no active construction drains.
    pub fn idle_tick(&mut self) {
        self.tick_when_build_target(0.0, 0.0, 0.0, 0.0);
    }

    /// Advance one second while building a target with the given drains.
    ///
    /// `f` is the effective build factor for this tick (0..=1). The economy
    /// update uses the full construction drains scaled by `f`, exactly like the
    /// ECS simulator.
    pub fn tick_when_build_target(
        &mut self,
        build_power: f64,
        unit_mass: f64,
        unit_energy: f64,
        unit_build_time: f64,
    ) {
        let drain_ratio = unit_build_time / build_power;
        self.mass_drain_per_second = unit_mass / drain_ratio;
        self.energy_drain_per_second = unit_energy / drain_ratio;

        let player_net_mass_income = self.player_net_mass_income();
        let player_net_energy_income = self.player_net_energy_income();

        self.eco_metrics.mass_storage_current += player_net_mass_income;
        self.eco_metrics.energy_storage_current += player_net_energy_income;

        self.time += 1.0;
    }

    // /// Add a completed target's economy contributions to the running state.
    // pub fn add_target_contributions(&mut self, target: &UnitEcoStats) {
    //     self.production_per_second_mass +=
    //         target.production_per_second_mass * target.adjacency.mass_production_multiplier();
    //     self.production_per_second_energy +=
    //         target.production_per_second_energy * target.adjacency.energy_production_multiplier();
    //     self.maintenance_consumption_per_second_energy +=
    //         target.maintenance_consumption_per_second_energy;
    //     self.mass_storage_cap += target.mass_storage;
    //     self.energy_storage_cap += target.energy_storage;
    // }
}

pub fn clamp(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}
