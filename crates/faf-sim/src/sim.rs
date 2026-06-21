//! Simple sequential simulator for FAF build orders.
//!
//! This simulator is intentionally naive: it builds one unit at a time in the
//! order given, assigning all available build power to the current project.
//! It is useful as a baseline and for validating the continuous-drain economy
//! model before adding concurrency and optimization.

use faf_units::{DataIndex, Unit};

use crate::economy::{BuildProject, EconomyState, RequestedBuildPower};

/// A single event in the simulated build timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildEvent {
    /// In-game seconds when the unit completed.
    pub time: f64,
    /// Blueprint id of the completed unit.
    pub unit_id: String,
    /// English unit name, if available.
    pub unit_name: Option<String>,
}

/// A sequential simulator that builds one unit at a time.
#[derive(Debug, Clone)]
pub struct SimpleSimulator<'a> {
    /// Units currently owned and available to build things.
    pub owned_units: Vec<&'a Unit>,
    /// Current economy state.
    pub state: EconomyState,
    /// Current simulation time in seconds.
    pub current_time: f64,
    /// Fixed timestep in seconds.
    pub dt: f64,
}

impl<'a> SimpleSimulator<'a> {
    /// Create a simulator from starting units.
    ///
    /// The initial economy is derived by summing production and storage from
    /// the starting units.
    pub fn new(_index: &'a DataIndex, starting_units: Vec<&'a Unit>, dt: f64) -> Self {
        let state = derive_economy(&starting_units);
        Self {
            owned_units: starting_units,
            state,
            current_time: 0.0,
            dt,
        }
    }

    /// Simulate building a sequence of units one after another.
    ///
    /// Returns the list of completion events. Each completed unit becomes
    /// available as a builder for subsequent projects.
    pub fn simulate_sequence(&mut self, sequence: &[&'a Unit]) -> Vec<BuildEvent> {
        let mut events = Vec::with_capacity(sequence.len());
        for target in sequence {
            let event = self.build_unit(target);
            self.owned_units.push(target);
            // Recompute economy since the new unit may produce/store resources.
            self.state = derive_economy(&self.owned_units);
            events.push(event);
        }
        events
    }

    /// Build a single unit using all currently available build power.
    fn build_unit(&mut self, target: &'a Unit) -> BuildEvent {
        let mut project = BuildProject::new(target).expect("unit must have economy data");
        project.assigned_build_power = self.available_build_power();

        loop {
            let outcome = project.tick(&mut self.state, self.dt);
            self.current_time += self.dt;

            if outcome.is_completed() {
                break;
            }

            // Safety valve: abort if we somehow never finish.
            if self.current_time > 1_000_000.0 {
                panic!(
                    "simulation exceeded time limit while building {}",
                    target.id
                );
            }
        }

        BuildEvent {
            time: self.current_time,
            unit_id: target.id.clone(),
            unit_name: target.name().map(|s| s.to_string()),
        }
    }

    /// Total build power from all owned units.
    fn available_build_power(&self) -> RequestedBuildPower {
        RequestedBuildPower(
            self.owned_units
                .iter()
                .filter_map(|u| u.economy.as_ref()?.build_rate)
                .sum(),
        )
    }
}

/// Derive an economy state by summing production and storage across units.
///
/// This is a simplified model: it ignores that production structures like
/// mass extractors or power generators would need to be built separately.
pub fn derive_economy(units: &[&Unit]) -> EconomyState {
    let mut net_mass_income = 0.0;
    let mut net_energy_income = 0.0;
    let mut mass_storage = 0.0;
    let mut energy_storage = 0.0;

    for unit in units {
        if let Some(econ) = &unit.economy {
            net_mass_income += econ.production_per_second_mass.unwrap_or(0.0);
            net_energy_income += econ.production_per_second_energy.unwrap_or(0.0);
            mass_storage += econ.storage_mass.unwrap_or(0.0);
            energy_storage += econ.storage_energy.unwrap_or(0.0);
        }
    }

    EconomyState {
        net_mass_income,
        net_energy_income,
        mass_storage,
        energy_storage,
        mass_storage_cap: mass_storage,
        energy_storage_cap: energy_storage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn simulate_building_t1_engineer_with_acu() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let t1_eng = index.find_unit("URL0105").expect("T1 engineer exists");

        let mut sim = SimpleSimulator::new(&index, vec![acu], 1.0);
        let events = sim.simulate_sequence(&[t1_eng]);

        assert_eq!(events.len(), 1);
        let build_time = t1_eng.economy.as_ref().unwrap().build_time.unwrap();
        let build_power = acu.economy.as_ref().unwrap().build_rate.unwrap();
        let expected_time = build_time / build_power;

        assert!((events[0].time - expected_time).abs() < 1.0);
        assert_eq!(events[0].unit_id, "URL0105");
    }

    #[test]
    fn simulate_t1_factory_then_t1_engineer() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let t1_factory = index.find_unit("URB0101").expect("T1 factory exists");
        let t1_eng = index.find_unit("URL0105").expect("T1 engineer exists");

        let mut sim = SimpleSimulator::new(&index, vec![acu], 1.0);
        let events = sim.simulate_sequence(&[t1_factory, t1_eng]);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].unit_id, "URB0101");
        assert_eq!(events[1].unit_id, "URL0105");

        // The T1 engineer should finish after the factory plus its own build time.
        assert!(events[1].time > events[0].time);
    }
}
