//! Solver state and per-tick economy updates.

use faf_blueprints::UnitEcoStats;
use faf_sim_shared::EcoSnapshot;

pub(crate) const EPS: f64 = 1e-9;

/// Economy state tracked by the single-task solver.
///
/// Fields mirror the values carried by the ECS simulator's `EcoSnapshot` event,
/// but use plain `f64`s so the solver can run without Bevy overhead.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SolverState {
    pub(crate) time: f64,
    pub(crate) mass: f64,
    pub(crate) energy: f64,
    pub(crate) mass_income: f64,
    pub(crate) energy_income: f64,
    pub(crate) maintenance: f64,
    pub(crate) mass_cap: f64,
    pub(crate) energy_cap: f64,
}

impl SolverState {
    pub(crate) fn from_snapshot(snapshot: &EcoSnapshot) -> Self {
        Self {
            time: snapshot.time.value(),
            mass: snapshot.mass_storage.value(),
            energy: snapshot.energy_storage.value(),
            mass_income: snapshot.production_per_second_mass.value(),
            energy_income: snapshot.production_per_second_energy.value(),
            maintenance: snapshot.maintenance_consumption_per_second_energy.value(),
            mass_cap: snapshot.mass_storage_cap.value(),
            energy_cap: snapshot.energy_storage_cap.value(),
        }
    }

    pub(crate) fn is_depleted(&self) -> bool {
        self.energy < self.maintenance
    }

    /// Mass income scaled by energy efficiency when energy is depleted.
    ///
    /// `extra_drain` is the energy consumed by construction this tick; it is
    /// `0.0` for idle ticks.
    fn scaled_mass_income(&self, extra_drain: f64) -> f64 {
        let eff = if self.is_depleted() {
            (self.energy_income / (self.maintenance + extra_drain)).min(1.0)
        } else {
            1.0
        };
        self.mass_income * eff
    }

    /// Advance one second with no active construction drains.
    pub(crate) fn idle_tick(&mut self) {
        let mm_scaled = self.scaled_mass_income(0.0);

        self.energy = clamp(
            self.energy + self.energy_income - self.maintenance,
            0.0,
            self.energy_cap,
        );
        self.mass = clamp(self.mass + mm_scaled, 0.0, self.mass_cap);
        self.time += 1.0;
    }

    /// Advance one second while building a target with the given drains.
    ///
    /// `f` is the effective build factor for this tick (0..=1). The economy
    /// update uses the full construction drains scaled by `f`, exactly like the
    /// ECS simulator.
    pub(crate) fn target_tick(&mut self, mass_drain: f64, energy_drain: f64, f: f64) {
        let mm_scaled = self.scaled_mass_income(energy_drain);

        self.energy = clamp(
            self.energy + self.energy_income - self.maintenance - f * energy_drain,
            0.0,
            self.energy_cap,
        );
        self.mass = clamp(self.mass + mm_scaled - f * mass_drain, 0.0, self.mass_cap);
        self.time += 1.0;
    }

    /// Add a completed target's economy contributions to the running state.
    pub(crate) fn add_target_contributions(&mut self, target: &UnitEcoStats) {
        self.mass_income +=
            target.production_per_second_mass * target.adjacency.mass_production_multiplier();
        self.energy_income +=
            target.production_per_second_energy * target.adjacency.energy_production_multiplier();
        self.maintenance += target.maintenance_consumption_per_second_energy;
        self.mass_cap += target.mass_storage;
        self.energy_cap += target.energy_storage;
    }

    /// Convert the solver's internal state into the public `EcoSnapshot` shape.
    ///
    /// Drain and spent totals are not tracked by the solver, so they are set to
    /// zero.
    pub(crate) fn to_snapshot(&self) -> EcoSnapshot {
        use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Time};

        EcoSnapshot {
            time: Time::from_raw(self.time),
            production_per_second_mass: MassRate::from_raw(self.mass_income),
            production_per_second_energy: EnergyRate::from_raw(self.energy_income),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(self.maintenance),
            mass_drain: MassRate::from_raw(0.0),
            energy_drain: EnergyRate::from_raw(0.0),
            total_mass_spent: Mass::from_raw(0.0),
            total_energy_spent: Energy::from_raw(0.0),
            mass_storage: Mass::from_raw(self.mass),
            mass_storage_cap: Mass::from_raw(self.mass_cap),
            energy_storage: Energy::from_raw(self.energy),
            energy_storage_cap: Energy::from_raw(self.energy_cap),
        }
    }
}

pub(crate) fn clamp(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}
