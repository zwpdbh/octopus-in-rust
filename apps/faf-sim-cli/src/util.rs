//! Small shared helpers used by multiple CLI commands.

use std::path::PathBuf;

use faf_sim_shared::{EcoSnapshot, EconomyRuntimeState};

/// Read and deserialize a JSON file, exiting the process on failure.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", path.display(), e);
        std::process::exit(1);
    });
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("Failed to parse {}: {}", path.display(), e);
        std::process::exit(1);
    })
}

/// Derive an [`EcoSnapshot`] from the [`EconomyRuntimeState`] embedded in a plan.
///
/// Missing fields (drains, accumulated totals, capacities) are filled with
/// sensible defaults so the predictor can run from a plan file alone.
pub fn eco_snapshot_from_runtime_state(state: &EconomyRuntimeState) -> EcoSnapshot {
    EcoSnapshot {
        time: 0.0,
        production_per_second_mass: state.production_per_second_mass.value(),
        production_per_second_energy: state.production_per_second_energy.value(),
        maintenance_consumption_per_second_energy: state
            .maintenance_consumption_per_second_energy
            .value(),
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage: state.mass_storage.current.value(),
        mass_storage_cap: state.mass_storage.cap.value(),
        energy_storage: state.energy_storage.current.value(),
        energy_storage_cap: state.energy_storage.cap.value(),
    }
}
