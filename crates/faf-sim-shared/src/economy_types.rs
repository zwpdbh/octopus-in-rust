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
    pub initial_eco: GameEcoParameters,
    /// Tasks to run, in queue order.
    pub tasks: Vec<BuildTask>,
}

/// `EcoSnapshot` is a flat, primitive view of one tick and includes construction
/// drain rates; `EconomyRuntimeState` is the typed, evolving state that the
/// simulator mutates to produce those snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GameEcoParameters {
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

impl Default for GameEcoParameters {
    fn default() -> Self {
        Self {
            production_per_second_mass: MassRate::from_raw(1.0),
            production_per_second_energy: EnergyRate::from_raw(20.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_storage: Storage::new(Mass::from_raw(650.0), Mass::from_raw(650.0)),
            energy_storage: Storage::new(Energy::from_raw(4000.0), Energy::from_raw(4000.0)),
        }
    }
}

impl GameEcoParameters {
    /// Energy available for construction after paying maintenance.
    pub fn energy_available(s: &Self) -> f64 {
        s.production_per_second_energy.value() - s.maintenance_consumption_per_second_energy.value()
    }

    /// Net energy change per second (income − maintenance − drain).
    pub fn energy_net(s: &Self, energy_drain: EnergyRate) -> f64 {
        GameEcoParameters::energy_available(&s) - energy_drain.value()
    }

    /// FAF army-wide energy efficiency ratio used to scale mass income.
    pub fn energy_efficiency(s: &Self, energy_drain: EnergyRate) -> f64 {
        let requested = s.maintenance_consumption_per_second_energy.value() + energy_drain.value();
        if requested <= 0.0 {
            1.0
        } else {
            (s.production_per_second_energy.value() / requested).min(1.0)
        }
    }

    /// Mass income after applying FAF energy-stall scaling.
    pub fn scaled_mass_income(s: &Self, energy_drain: EnergyRate) -> f64 {
        if s.energy_storage.current.value() < s.maintenance_consumption_per_second_energy.value() {
            s.production_per_second_mass.value()
                * GameEcoParameters::energy_efficiency(s, energy_drain)
        } else {
            s.production_per_second_mass.value()
        }
    }

    /// Net mass change per second (scaled income − drain).
    pub fn mass_net(s: &Self, mass_drain: MassRate, energy_drain: EnergyRate) -> f64 {
        GameEcoParameters::scaled_mass_income(s, energy_drain) - mass_drain.value()
    }

    /// True when FAF would scale mass production because energy storage is below
    /// total maintenance.
    pub fn mass_scaling_active(s: &Self, energy_drain: EnergyRate) -> bool {
        if s.production_per_second_energy.value()
            < (s.maintenance_consumption_per_second_energy.value() + energy_drain.value())
        {
            return s.energy_storage.current.value() < 0.0;
        }
        false
    }
}

/// A EcoSnapshot is GameEcoParameters + Time
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoSnapshot {
    pub time: f64,
    pub mass_storage_current: f64,
    pub energy_storage_current: f64,
    pub production_per_second_mass: f64,
    pub production_per_second_energy: f64,
    pub maintenance_consumption_per_second_energy: f64,
    pub mass_storage_cap: f64,
    pub energy_storage_cap: f64,
}

pub const EPS: f64 = 1e-9;

impl EcoSnapshot {
    pub fn to_game_eco_parameters(self: &Self) -> GameEcoParameters {
        todo!("not implemented")
    }

    pub fn init_with_eco_parameters(game_eco_parameters: &GameEcoParameters) -> Self {
        Self {
            time: 0.0,
            mass_storage_current: game_eco_parameters.mass_storage.current.value(),
            energy_storage_current: game_eco_parameters.energy_storage.current.value(),
            production_per_second_mass: game_eco_parameters.production_per_second_mass.value(),
            production_per_second_energy: game_eco_parameters.production_per_second_energy.value(),
            maintenance_consumption_per_second_energy: game_eco_parameters
                .maintenance_consumption_per_second_energy
                .value(),
            mass_storage_cap: game_eco_parameters.mass_storage.cap.value(),
            energy_storage_cap: game_eco_parameters.energy_storage.cap.value(),
        }
    }

    pub fn is_depleted(&self) -> bool {
        self.energy_storage_current < self.maintenance_consumption_per_second_energy
    }

    /// Mass income scaled by energy efficiency when energy is depleted.
    ///
    /// `extra_drain` is the energy consumed by construction this tick; it is
    /// `0.0` for idle ticks.
    fn scaled_mass_income(&self, extra_drain: f64) -> f64 {
        let eff = if self.is_depleted() {
            (self.production_per_second_energy
                / (self.maintenance_consumption_per_second_energy + extra_drain))
                .min(1.0)
        } else {
            1.0
        };
        self.production_per_second_mass * eff
    }

    /// Advance one second with no active construction drains.
    pub fn idle_tick(&mut self) {
        let mm_scaled = self.scaled_mass_income(0.0);

        self.energy_storage_current = clamp(
            self.energy_storage_current + self.production_per_second_energy
                - self.maintenance_consumption_per_second_energy,
            0.0,
            self.energy_storage_cap,
        );
        self.mass_storage_current = clamp(
            self.mass_storage_current + mm_scaled,
            0.0,
            self.mass_storage_cap,
        );
        self.time += 1.0;
    }

    /// Advance one second while building a target with the given drains.
    ///
    /// `f` is the effective build factor for this tick (0..=1). The economy
    /// update uses the full construction drains scaled by `f`, exactly like the
    /// ECS simulator.
    pub fn target_tick(&mut self, mass_drain: f64, energy_drain: f64, f: f64) {
        let mm_scaled = self.scaled_mass_income(energy_drain);

        self.energy_storage_current = clamp(
            self.energy_storage_current + self.production_per_second_energy
                - self.maintenance_consumption_per_second_energy
                - f * energy_drain,
            0.0,
            self.energy_storage_cap,
        );
        self.mass_storage_current = clamp(
            self.mass_storage_current + mm_scaled - f * mass_drain,
            0.0,
            self.mass_storage_cap,
        );
        self.time += 1.0;
    }

    /// Add a completed target's economy contributions to the running state.
    pub fn add_target_contributions(&mut self, target: &UnitEcoStats) {
        self.production_per_second_mass +=
            target.production_per_second_mass * target.adjacency.mass_production_multiplier();
        self.production_per_second_energy +=
            target.production_per_second_energy * target.adjacency.energy_production_multiplier();
        self.maintenance_consumption_per_second_energy +=
            target.maintenance_consumption_per_second_energy;
        self.mass_storage_cap += target.mass_storage;
        self.energy_storage_cap += target.energy_storage;
    }
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
