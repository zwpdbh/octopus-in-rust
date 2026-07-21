//! Derived metrics computed from an [`EcoSnapshot`].
//!
//! These helpers are shared between the web UI, the CLI, and any other consumer
//! that wants to visualize the relationships in the raw snapshot data without
//! reimplementing the FAF economy rules.

use crate::runtime::EcoSnapshot;

/// Energy available for construction after paying maintenance.
pub fn energy_available(s: &EcoSnapshot) -> f64 {
    s.production_per_second_energy.value() - s.maintenance_consumption_per_second_energy.value()
}

/// Net energy change per second (income − maintenance − drain).
pub fn energy_net(s: &EcoSnapshot) -> f64 {
    energy_available(s) - s.energy_drain.value()
}

/// FAF army-wide energy efficiency ratio used to scale mass income.
pub fn energy_efficiency(s: &EcoSnapshot) -> f64 {
    let requested = s.maintenance_consumption_per_second_energy.value() + s.energy_drain.value();
    if requested <= 0.0 {
        1.0
    } else {
        (s.production_per_second_energy.value() / requested).min(1.0)
    }
}

/// Mass income after applying FAF energy-stall scaling.
pub fn scaled_mass_income(s: &EcoSnapshot) -> f64 {
    if s.energy_storage.value() < s.maintenance_consumption_per_second_energy.value() {
        s.production_per_second_mass.value() * energy_efficiency(s)
    } else {
        s.production_per_second_mass.value()
    }
}

/// Net mass change per second (scaled income − drain).
pub fn mass_net(s: &EcoSnapshot) -> f64 {
    scaled_mass_income(s) - s.mass_drain.value()
}

/// True when FAF would scale mass production because energy storage is below
/// total maintenance.
pub fn mass_scaling_active(s: &EcoSnapshot) -> bool {
    s.energy_storage.value() < s.maintenance_consumption_per_second_energy.value()
}
