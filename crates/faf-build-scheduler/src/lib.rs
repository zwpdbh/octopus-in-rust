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
pub mod search;
pub mod util;

pub use algorithms::{algorithm_by_kind, AlgorithmKind, Greedy, SchedulingAlgorithm};
pub use request::{
    EcoScheduleInput, EcoScheduleRequest, EcoTarget, SearchOptions, UnitScheduleInput,
    UnitScheduleRequest,
};
pub use result::{Action, Schedule, ScheduleError, StepResult};
pub use scheduler::Scheduler;

#[cfg(test)]
mod tests {
    use super::*;
    use faf_blueprints::{TechLevel, UnitKind};
    use faf_sim::quantities::MassRate;
    use faf_sim::runtime::EcoSnapshot;

    fn test_library() -> faf_blueprints::BlueprintLibrary {
        faf_blueprints::BlueprintLibrary::from_default_units().expect("load default units")
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
    #[should_panic(expected = "implement greedy unit scheduling")]
    fn greedy_unit_schedule_is_todo() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = UnitScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: UnitKind::Engineer(TechLevel::T1),
            options: SearchOptions::default(),
        };

        let _ = scheduler.schedule_unit(&request);
    }

    #[test]
    fn greedy_eco_schedule_reaches_target() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = EcoScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: EcoTarget {
                mass_production: MassRate::from_raw(7.0),
                tolerance: 1.0,
            },
            options: SearchOptions::default(),
        };

        let schedule = scheduler
            .schedule_eco(&request)
            .expect("schedule should succeed");
        assert!(!schedule.steps.is_empty(), "schedule should contain steps");
        assert!(
            schedule.final_eco.production_per_second_mass >= 7.0,
            "final mass production should meet target"
        );
    }

    #[test]
    fn greedy_eco_schedule_builds_multiple_steps() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = EcoScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: EcoTarget {
                mass_production: MassRate::from_raw(15.0),
                tolerance: 1.0,
            },
            options: SearchOptions::default(),
        };

        let schedule = scheduler
            .schedule_eco(&request)
            .expect("schedule should succeed");
        assert!(
            schedule.steps.len() >= 3,
            "should need at least three mass extractors"
        );
        assert!(
            schedule.final_eco.production_per_second_mass >= 15.0,
            "final mass production should meet target"
        );
    }
}
