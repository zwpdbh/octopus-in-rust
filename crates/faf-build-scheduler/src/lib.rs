//! Build-order scheduler for FAF.
//!
//! This crate exposes a pluggable scheduling abstraction over the `faf-sim`
//! blueprint library and solver. The default implementation is a placeholder
//! that proves the wiring; real search algorithms can be added behind the same
//! [`SchedulingAlgorithm`] trait.

pub mod algorithms;
pub mod request;
pub mod result;
pub mod scheduler;
pub mod util;

pub use algorithms::{algorithm_by_kind, AlgorithmKind, Placeholder, SchedulingAlgorithm};
pub use request::{
    EcoScheduleInput, EcoScheduleRequest, EcoTarget, SearchOptions, UnitScheduleInput,
    UnitScheduleRequest,
};
pub use result::{Action, Schedule, ScheduleError, StepResult};
pub use scheduler::Scheduler;

#[cfg(test)]
mod tests {
    use super::*;
    use faf_sim::runtime::EcoSnapshot;
    use faf_sim::units::{TechLevel, UnitKind};
    use std::path::PathBuf;

    fn test_library() -> faf_sim::units::BlueprintLibrary {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let units_file = manifest.join("../../plugins/faf-units/data/faf_units.json");
        let text = std::fs::read_to_string(&units_file).expect("read units file");
        let index: faf_units::DataIndex = serde_json::from_str(&text).expect("parse units file");
        faf_sim::units::BlueprintLibrary::new(index)
    }

    fn default_eco() -> EcoSnapshot {
        EcoSnapshot {
            time: 0.0,
            production_per_second_mass: 5.0,
            production_per_second_energy: 50.0,
            maintenance_consumption_per_second_energy: 0.0,
            mass_drain: 0.0,
            energy_drain: 0.0,
            total_mass_spent: 0.0,
            total_energy_spent: 0.0,
            mass_storage: 2000.0,
            mass_storage_cap: 2000.0,
            energy_storage: 4000.0,
            energy_storage_cap: 4000.0,
        }
    }

    #[test]
    fn placeholder_unit_schedule_builds_t1_engineer() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = UnitScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: UnitKind::Engineer(TechLevel::T1),
            options: SearchOptions::default(),
        };

        let schedule = scheduler.schedule_unit(&request).expect("schedule");
        assert!(!schedule.plan.items.is_empty());
        assert!(schedule.total_time_seconds < 6000.0);
        assert!(schedule.final_eco.production_per_second_mass >= 0.0);
        assert!(!schedule.to_build_queue().tasks.is_empty());
    }

    #[test]
    fn placeholder_eco_schedule_reaches_energy_target() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = EcoScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: EcoTarget {
                mass_production: None,
                energy_production: Some(70.0),
                mass_storage_cap: None,
                energy_storage_cap: None,
                tolerance: 1.0,
            },
            options: SearchOptions::default(),
        };

        let schedule = scheduler.schedule_eco(&request).expect("schedule");
        assert!(!schedule.plan.items.is_empty());
        assert!(schedule.final_eco.production_per_second_energy + 1.0 >= 70.0);
        assert!(!schedule.to_build_queue().tasks.is_empty());
    }

    #[test]
    fn unimplemented_algorithm_reports_error() {
        let library = test_library();
        let scheduler = Scheduler::with_algorithm(library, AlgorithmKind::BeamSearch);
        let request = UnitScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: UnitKind::Engineer(TechLevel::T1),
            options: SearchOptions::default(),
        };

        let err = scheduler.schedule_unit(&request).expect_err("should fail");
        assert!(matches!(
            err,
            ScheduleError::AlgorithmNotImplemented(AlgorithmKind::BeamSearch)
        ));
    }
}
