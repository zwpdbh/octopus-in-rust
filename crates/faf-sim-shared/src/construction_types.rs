//! Shared, serializable input/output types for the FAF simulator and solver.
//!
//! These types are deliberately lightweight so that callers (the Dioxus web app,
//! the CLI, tests, the WebSocket service, and the analytical solver) can describe
//! units and queues without depending on the full ECS runtime.

use faf_blueprints::UnitEcoStats;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// pub struct ConstructionBuilder {
//     unit_id: String,
// }

// impl ConstructionBuilder {
//     fn get_build_power(&self) -> f64 {
//         todo!("given unit id, from faf-game-units get its bp")
//     }
// }

/// One task in a build queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstructionTask {
    /// Caller-defined id, echoed back in start/complete events.
    pub id: Uuid,
    /// Delay after the previous task finishes before this task may begin.
    ///
    /// For the first task this is a delay relative to simulation start (time 0).
    #[serde(default = "default_start_after")]
    pub start_after: usize,

    /// Builders assigned to the task.
    pub builders: Vec<UnitEcoStats>,
    /// Units being built, in order. Builders work through the list sequentially.
    pub target: Vec<UnitEcoStats>,
}

fn default_start_after() -> usize {
    0
}

/// A full build queue to simulate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildQueue {
    /// Initial economy state (income and storage).
    pub initial_eco: PlayerEcoMetrics,
    /// Tasks to run, in queue order.
    pub tasks: Vec<ConstructionTask>,
}

/// `EcoSnapshot` is a flat, primitive view of one tick and includes construction
/// drain rates; `EconomyRuntimeState` is the typed, evolving state that the
/// simulator mutates to produce those snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerEcoMetrics {
    pub mass_generate_rate: f64,
    pub mass_drain: f64,
    pub energy_generate_rate: f64,
    pub energy_drain: f64,

    pub mass_in_storage: f64,
    pub max_capacity_in_mass_storage: f64,

    pub energy_in_storage: f64,
    pub max_capacity_in_energy_storage: f64,
}

impl Default for PlayerEcoMetrics {
    fn default() -> Self {
        Self {
            mass_generate_rate: 1.0,
            mass_drain: 0.0,
            energy_generate_rate: 20.0,
            energy_drain: 0.0,
            mass_in_storage: 650.0,
            max_capacity_in_mass_storage: 650.0,
            energy_in_storage: 4000.0,
            max_capacity_in_energy_storage: 4000.0,
        }
    }
}

impl PlayerEcoMetrics {
    /// FAF army-wide energy efficiency ratio used to scale mass income.
    pub fn energy_efficiency(&self, energy_drain: f64) -> f64 {
        let requested = self.energy_drain + energy_drain;
        if requested <= 0.0 {
            1.0
        } else {
            (self.energy_generate_rate / requested).min(1.0)
        }
    }

    pub fn mass_efficiency(&self, energy_drain: f64) -> f64 {
        let net_energy_income = self.energy_generate_rate - self.energy_drain - energy_drain;
        if self.energy_in_storage + net_energy_income < 0.0 {
            self.energy_efficiency(energy_drain)
        } else {
            1.0
        }
    }
}

/// A EcoSnapshot is GameEcoParameters + Time
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerEcoSnapshot {
    pub time: f64,
    pub eco_metrics: PlayerEcoMetrics,
    pub mass_drain_per_second: f64,
    pub energy_drain_per_second: f64,
}

pub const EPS: f64 = 1e-9;

impl PlayerEcoSnapshot {
    pub fn init_with_eco_parameters(game_eco_metrics: &PlayerEcoMetrics) -> Self {
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
        let actual_mass_production_per_second = self.eco_metrics.mass_generate_rate
            * self
                .eco_metrics
                .mass_efficiency(self.energy_drain_per_second);
        actual_mass_production_per_second - self.mass_drain_per_second
    }

    fn player_net_energy_income(&self) -> f64 {
        self.eco_metrics.energy_generate_rate
            - self.eco_metrics.energy_drain
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

        self.eco_metrics.mass_in_storage += player_net_mass_income;
        self.eco_metrics.energy_in_storage += player_net_energy_income;

        self.time += 1.0;
    }
}
