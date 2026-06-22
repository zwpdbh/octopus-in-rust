//! Economy state derivation for FAF build-order simulation.
//!
//! This module provides the bridge from a collection of owned units to the
//! observable economy state (income, storage, net flow).

use faf_units::Unit;

use crate::economy::{summarize_economy, EconomyState};

/// A single event in the simulated build timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildEvent {
    /// In-game seconds when the unit completed.
    pub time: f64,
    /// Blueprint id of the completed unit.
    pub unit_id: String,
    /// Display name for the completed unit.
    pub unit_name: String,
}

/// Derive an economy state by summing production, consumption, and storage
/// across units.
///
/// Net income is production minus maintenance consumption. It may be negative,
/// matching the in-game economy display.
pub fn derive_economy(units: &[&Unit]) -> EconomyState {
    let mut mass_storage = 0.0;
    let mut energy_storage = 0.0;

    for unit in units {
        if let Some(econ) = &unit.economy {
            mass_storage += econ.storage_mass.unwrap_or(0.0);
            energy_storage += econ.storage_energy.unwrap_or(0.0);
        }
    }

    let net = summarize_economy(units, &[]);

    EconomyState {
        net_mass_income: net.mass_per_second,
        net_energy_income: net.energy_per_second,
        mass_storage,
        energy_storage,
        mass_storage_cap: mass_storage,
        energy_storage_cap: energy_storage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_units::DataIndex;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn acu_starting_economy() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let state = derive_economy(&[acu]);

        assert!((state.net_mass_income - 1.0).abs() < 1e-9);
        assert!((state.net_energy_income - 20.0).abs() < 1e-9);
        assert!((state.mass_storage - 650.0).abs() < 1e-9);
        assert!((state.energy_storage - 3900.0).abs() < 1e-9);
    }

    #[test]
    fn derive_economy_subtracts_maintenance() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let t1_mex = index.find_unit("URB1103").expect("T1 mex exists");

        let state = derive_economy(&[acu, t1_mex]);

        // ACU: +1 mass/s, +20 energy/s. T1 mex: +2 mass/s, -2 energy/s maintenance.
        assert!((state.net_mass_income - 3.0).abs() < 1e-9);
        assert!((state.net_energy_income - 18.0).abs() < 1e-9);
    }
}
