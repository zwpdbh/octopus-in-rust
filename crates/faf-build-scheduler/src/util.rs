//! Shared helpers for the scheduler.

use std::collections::HashMap;

use faf_blueprints::{BlueprintLibrary, UnitKind, UnitRole};
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage};
use faf_sim_shared::{EcoSnapshot, EconomyRuntimeState};

/// Returns true if the unit kind is a mass extractor (including capped).
pub(crate) fn is_mex(library: &BlueprintLibrary, kind: &UnitKind) -> bool {
    library.role(kind) == UnitRole::MassExtractor
}

/// Counts how many mass extractors are in the inventory.
pub(crate) fn count_mex(inventory: &HashMap<UnitKind, u32>, library: &BlueprintLibrary) -> u32 {
    inventory
        .iter()
        .filter(|(kind, _)| is_mex(library, kind))
        .map(|(_, count)| *count)
        .sum()
}

/// Convert a flat snapshot into the simulator's typed runtime state.
pub fn eco_snapshot_to_runtime_state(snapshot: &EcoSnapshot) -> EconomyRuntimeState {
    EconomyRuntimeState {
        production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
        production_per_second_energy: EnergyRate::from_raw(snapshot.production_per_second_energy),
        maintenance_consumption_per_second_energy: EnergyRate::from_raw(
            snapshot.maintenance_consumption_per_second_energy,
        ),
        mass_storage: Storage {
            current: Mass::from_raw(snapshot.mass_storage),
            cap: Mass::from_raw(snapshot.mass_storage_cap),
        },
        energy_storage: Storage {
            current: Energy::from_raw(snapshot.energy_storage),
            cap: Energy::from_raw(snapshot.energy_storage_cap),
        },
    }
}

/// Convert a typed runtime state back into a flat snapshot.
pub fn eco_runtime_state_to_snapshot(state: &EconomyRuntimeState) -> EcoSnapshot {
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
