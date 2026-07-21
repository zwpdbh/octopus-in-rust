//! Shared helpers for the scheduler.

use faf_blueprints::{BlueprintLibrary, UnitKind, UnitRole};
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
use faf_sim_shared::{EcoSnapshot, EconomyRuntimeState};

/// Returns true if the unit kind is a mass extractor (including capped).
pub(crate) fn is_mex(library: &BlueprintLibrary, kind: &UnitKind) -> bool {
    library.role(kind) == UnitRole::MassExtractor
}

/// Counts how many mass extractors are in an iterator of unit kinds.
pub(crate) fn count_mex_from_iter<'a>(
    kinds: impl IntoIterator<Item = &'a UnitKind>,
    library: &BlueprintLibrary,
) -> u32 {
    kinds
        .into_iter()
        .filter(|kind| is_mex(library, kind))
        .count() as u32
}

/// Convert a flat snapshot into the simulator's typed runtime state.
pub fn eco_snapshot_to_runtime_state(snapshot: &EcoSnapshot) -> EconomyRuntimeState {
    EconomyRuntimeState {
        production_per_second_mass: snapshot.production_per_second_mass,
        production_per_second_energy: snapshot.production_per_second_energy,
        maintenance_consumption_per_second_energy: snapshot
            .maintenance_consumption_per_second_energy,
        mass_storage: Storage {
            current: snapshot.mass_storage,
            cap: snapshot.mass_storage_cap,
        },
        energy_storage: Storage {
            current: snapshot.energy_storage,
            cap: snapshot.energy_storage_cap,
        },
    }
}

/// Convert a typed runtime state back into a flat snapshot.
pub fn eco_runtime_state_to_snapshot(state: &EconomyRuntimeState) -> EcoSnapshot {
    EcoSnapshot {
        time: Time::from_raw(0.0),
        production_per_second_mass: state.production_per_second_mass,
        production_per_second_energy: state.production_per_second_energy,
        maintenance_consumption_per_second_energy: state.maintenance_consumption_per_second_energy,
        mass_drain: MassRate::from_raw(0.0),
        energy_drain: EnergyRate::from_raw(0.0),
        total_mass_spent: Mass::from_raw(0.0),
        total_energy_spent: Energy::from_raw(0.0),
        mass_storage: state.mass_storage.current,
        mass_storage_cap: state.mass_storage.cap,
        energy_storage: state.energy_storage.current,
        energy_storage_cap: state.energy_storage.cap,
    }
}
