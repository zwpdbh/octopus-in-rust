//! Global economy tick that combines all active construction drains.
//!
//! This module implements the FAF-standard interaction between energy shortage
//! and mass production. When energy storage drops below total
//! `MaintenanceConsumptionPerSecondEnergy`, `ProductionPerSecondMass` is scaled
//! by the army-wide energy efficiency ratio.

use crate::quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};

use super::rules::GameEcoMetrics;

/// Result of applying a *global* drain to an economy state for one tick.
///
/// Unlike [`TickResult`](super::rules::TickResult), this models the combined
/// drain of all active projects and applies FAF-standard mass-income scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphTickResult {
    /// Global stall factor applied to every active project.
    pub effective_factor: f64,
    /// Mass actually consumed across all projects.
    pub mass_consumed: Mass,
    /// Energy actually consumed across all projects.
    pub energy_consumed: Energy,
    /// New mass storage after the tick.
    pub new_mass_storage: Storage<Mass>,
    /// New energy storage after the tick.
    pub new_energy_storage: Storage<Energy>,
    /// True if energy was the limiting resource.
    pub energy_stalled: bool,
    /// True if mass was the limiting resource.
    pub mass_stalled: bool,
    /// Net `ProductionPerSecondMass` after applying the energy-stall scaling and
    /// subtracting the actual mass drain.
    pub net_mass_income: MassRate,
    /// Net `ProductionPerSecondEnergy` after subtracting the actual energy drain.
    pub net_energy_income: EnergyRate,
}

/// Compute the effective global stall factor for the combined drain of all
/// active projects in the graph model.
///
/// Following FAF's standard, when energy storage is below total energy
/// maintenance, mass production is scaled by the army's energy efficiency:
/// `gross_energy_income / (maintenance_consumption_per_second_energy + construction_energy_drain)`.
/// This matches the scaling applied to mass extractors in FAF. The mass factor
/// is then recomputed with the scaled income, so mass can become the limiting
/// resource after energy shortage reduces it.
pub fn apply_tick_graph(
    total_mass_drain: f64,
    total_energy_drain: f64,
    state: &GameEcoMetrics,
    dt: f64,
) -> GraphTickResult {
    EconomyTick::new(state, total_mass_drain, total_energy_drain, dt).run()
}

/// A single global economy tick, broken into named steps.
///
/// The struct is intentionally cheap and immutable: it holds the inputs and
/// computes each intermediate value on demand, making the FAF rule easy to
/// follow and unit-test in isolation.
#[derive(Debug, Clone, Copy)]
struct EconomyTick<'a> {
    state: &'a GameEcoMetrics,
    mass_drain: f64,
    energy_drain: f64,
    dt: Time,
}

impl<'a> EconomyTick<'a> {
    fn new(state: &'a GameEcoMetrics, mass_drain: f64, energy_drain: f64, dt: f64) -> Self {
        Self {
            state,
            mass_drain,
            energy_drain,
            dt: Time::from_raw(dt),
        }
    }

    /// Gross energy income this tick.
    fn gross_energy_income(&self) -> Energy {
        self.state.production_energy_per_second * self.dt
    }

    /// Maintenance energy cost this tick.
    fn maintenance_energy(&self) -> Energy {
        self.state.maintenance_consumption_per_second_energy * self.dt
    }

    /// Energy available for construction this tick after paying maintenance.
    fn energy_available_for_construction(&self) -> Energy {
        (self.gross_energy_income() - self.maintenance_energy()).max(Energy::zero())
    }

    /// Mass requested by all construction this tick.
    fn requested_mass(&self) -> Mass {
        Mass::from_raw(self.mass_drain * self.dt.value())
    }

    /// Energy requested by all construction this tick.
    fn requested_energy(&self) -> Energy {
        Energy::from_raw(self.energy_drain * self.dt.value())
    }

    /// Fraction of construction energy demand that can be paid from storage +
    /// income left after maintenance. 1.0 means construction energy is fully covered.
    fn energy_factor(&self) -> f64 {
        let requested = self.requested_energy();
        if requested.value() <= 0.0 {
            return 1.0;
        }
        let available = (self.state.energy_storage.current
            + self.energy_available_for_construction())
        .max(Energy::zero());
        (available / requested).min(1.0)
    }

    /// True when energy storage is below total maintenance, the FAF condition
    /// for mass-production scaling.
    fn energy_storage_depleted(&self) -> bool {
        self.state.energy_storage.current.value()
            < self.state.maintenance_consumption_per_second_energy.value()
    }

    /// Army-wide energy efficiency: gross income divided by total energy
    /// requested (maintenance + construction), clamped to 1.0.
    fn energy_efficiency(&self) -> f64 {
        let gross = self.state.production_energy_per_second;
        let total_requested = self.state.maintenance_consumption_per_second_energy
            + EnergyRate::from_raw(self.energy_drain);
        if total_requested.value() <= 0.0 {
            return 1.0;
        }
        (gross / total_requested).min(1.0)
    }

    /// Mass production rate after applying the FAF energy-efficiency scaling.
    fn scaled_mass_income(&self) -> MassRate {
        if self.energy_storage_depleted() {
            self.state.production_per_second_mass * self.energy_efficiency()
        } else {
            self.state.production_per_second_mass
        }
    }

    /// `ProductionPerSecondMass` available this tick after FAF scaling.
    fn scaled_mass_income_amount(&self) -> Mass {
        self.scaled_mass_income() * self.dt
    }

    /// Fraction of construction mass demand that can be paid from storage +
    /// scaled `ProductionPerSecondMass`.
    fn mass_factor(&self) -> f64 {
        let requested = self.requested_mass();
        if requested.value() <= 0.0 {
            return 1.0;
        }
        let available =
            (self.state.mass_storage.current + self.scaled_mass_income_amount()).max(Mass::zero());
        (available / requested).min(1.0)
    }

    /// Overall construction throttle: the most constrained resource.
    fn effective_factor(&self) -> f64 {
        self.mass_factor().min(self.energy_factor())
    }

    /// True if FAF considers this tick energy-stalled.
    ///
    /// When storage is depleted, energy is the root cause even if mass becomes
    /// the immediate construction bottleneck after scaling.
    fn energy_stalled(&self) -> bool {
        self.effective_factor() < 1.0
            && (self.energy_storage_depleted() || self.energy_factor() <= self.mass_factor())
    }

    /// True if mass is the construction bottleneck and energy is not stalled.
    fn mass_stalled(&self) -> bool {
        self.effective_factor() < 1.0
            && !self.energy_storage_depleted()
            && self.mass_factor() <= self.energy_factor()
    }

    /// Run the tick and return the resulting state changes.
    fn run(&self) -> GraphTickResult {
        let effective_factor = self.effective_factor();
        let mass_consumed = self.requested_mass() * effective_factor;
        let energy_consumed = self.requested_energy() * effective_factor;

        let scaled_mass_income = self.scaled_mass_income_amount();
        let gross_energy_income = self.gross_energy_income();
        let maintenance_energy = self.maintenance_energy();

        let new_mass_current = (self.state.mass_storage.current + scaled_mass_income
            - mass_consumed)
            .min(self.state.mass_storage.cap)
            .max(Mass::zero());
        let new_energy_current = (self.state.energy_storage.current + gross_energy_income
            - maintenance_energy
            - energy_consumed)
            .min(self.state.energy_storage.cap)
            .max(Energy::zero());

        GraphTickResult {
            effective_factor,
            mass_consumed,
            energy_consumed,
            new_mass_storage: Storage::new(new_mass_current, self.state.mass_storage.cap),
            new_energy_storage: Storage::new(new_energy_current, self.state.energy_storage.cap),
            energy_stalled: self.energy_stalled(),
            mass_stalled: self.mass_stalled(),
            net_mass_income: self.scaled_mass_income()
                - MassRate::from_raw(mass_consumed.value() / self.dt.value()),
            net_energy_income: self.state.production_energy_per_second
                - self.state.maintenance_consumption_per_second_energy
                - EnergyRate::from_raw(energy_consumed.value() / self.dt.value()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantities::{MassRate, Storage};

    fn state_with_maintenance(
        production_per_second_mass: f64,
        production_per_second_energy: f64,
        maintenance_consumption_per_second_energy: f64,
        mass_storage: f64,
        energy_storage: f64,
    ) -> GameEcoMetrics {
        GameEcoMetrics {
            production_per_second_mass: MassRate::from_raw(production_per_second_mass),
            production_energy_per_second: EnergyRate::from_raw(production_per_second_energy),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(
                maintenance_consumption_per_second_energy,
            ),
            mass_storage: Storage::new(Mass::from_raw(mass_storage), Mass::from_raw(1000.0)),
            energy_storage: Storage::new(
                Energy::from_raw(energy_storage),
                Energy::from_raw(1000.0),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn mass_income_scales_when_energy_storage_depleted() {
        let state = state_with_maintenance(10.0, 5.0, 5.0, 0.0, 0.0);
        let result = apply_tick_graph(0.0, 10.0, &state, 1.0);

        // Energy efficiency = gross / (maintenance + drain) = 5 / (5 + 10) = 1/3.
        let expected = 10.0 * (5.0 / 15.0);
        assert!((result.net_mass_income.value() - expected).abs() < 1e-9);
        assert!(result.energy_stalled);
        assert!(!result.mass_stalled);
    }

    #[test]
    fn mass_income_not_scaled_when_storage_above_maintenance() {
        // Gross energy production exceeds maintenance + drain, so construction runs
        // at full power and mass income is not scaled.
        let state = state_with_maintenance(10.0, 20.0, 5.0, 0.0, 5.0);
        let result = apply_tick_graph(10.0, 10.0, &state, 1.0);

        assert!((result.effective_factor - 1.0).abs() < 1e-9);
        assert!(!result.energy_stalled);
        assert!(!result.mass_stalled);
    }

    #[test]
    fn energy_efficiency_is_clamped_to_one() {
        let state = state_with_maintenance(10.0, 100.0, 5.0, 0.0, 0.0);
        let result = apply_tick_graph(10.0, 10.0, &state, 1.0);

        // Efficiency would be 100/15 > 1, clamped to 1. `ProductionPerSecondMass` stays at 10.
        assert!((result.net_mass_income.value() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn maintenance_is_subtracted_from_energy_storage() {
        // Gross income (2) is lower than maintenance (5). Even with no
        // construction, storage should drain by the net deficit each tick.
        let state = GameEcoMetrics {
            production_energy_per_second: EnergyRate::from_raw(2.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(5.0),
            energy_storage: Storage::new(Energy::from_raw(4000.0), Energy::from_raw(5000.0)),
            ..Default::default()
        };
        let result = apply_tick_graph(0.0, 0.0, &state, 1.0);

        assert!((result.net_energy_income.value() - -3.0).abs() < 1e-9);
        assert!(
            (result.new_energy_storage.current.value() - 3997.0).abs() < 1e-9,
            "energy storage should decrease by maintenance minus gross income"
        );
    }
}
