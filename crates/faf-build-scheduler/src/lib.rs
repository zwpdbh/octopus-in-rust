//! Build-order scheduler for FAF.
//!
//! This crate exposes a pluggable scheduling abstraction over the `faf-sim`
//! blueprint library and solver. The default implementation is a placeholder
//! that proves the wiring; real search algorithms can be added behind the same
//! [`SchedulingAlgorithm`] trait.

pub mod algorithms;
pub mod app;
pub mod components;
pub mod config;
pub mod plugins;
pub mod request;
pub mod resources;
pub mod result;
pub mod scheduler;
pub mod search;
pub mod util;

pub use algorithms::{algorithm_by_kind, AlgorithmKind, Greedy, SchedulingAlgorithm};
pub use config::SchedulerConfig;
pub use plugins::{
    decide_direction::{CurrentEcoDirection, EcoDirection},
    observe::Observation,
    run_to_completion, EcoSchedulingPlugin, SchedulerLifecyclePlugin, SchedulerResult,
    SchedulerSet, SchedulerState, UnitSchedulingPlugin,
};
pub use request::{
    EcoScheduleInput, EcoScheduleRequest, EcoTarget, SearchOptions, UnitScheduleInput,
    UnitScheduleRequest,
};
pub use resources::{
    CurrentTechLevel, EconomyState, SchedulerClock, SearchGoal, SearchProgress, StepLog, TaskLog,
};
pub use result::{Action, Schedule, ScheduleError, StepResult};
pub use scheduler::Scheduler;

#[cfg(test)]
mod tests {
    use super::*;
    use faf_blueprints::{TechLevel, UnitKind};
    use faf_quantities::MassRate;
    use faf_sim_shared::EcoSnapshot;

    fn test_library() -> faf_blueprints::BlueprintLibrary {
        faf_blueprints::BlueprintLibrary::from_default_units().expect("load default units")
    }

    fn default_eco() -> EcoSnapshot {
        use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Time};

        EcoSnapshot {
            time: Time::from_raw(0.0),
            production_per_second_mass: MassRate::from_raw(5.0),
            production_per_second_energy: EnergyRate::from_raw(50.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_drain: MassRate::from_raw(0.0),
            energy_drain: EnergyRate::from_raw(0.0),
            total_mass_spent: Mass::from_raw(0.0),
            total_energy_spent: Energy::from_raw(0.0),
            mass_storage: Mass::from_raw(2000.0),
            mass_storage_cap: Mass::from_raw(2000.0),
            energy_storage: Energy::from_raw(4000.0),
            energy_storage_cap: Energy::from_raw(4000.0),
        }
    }

    #[test]
    fn greedy_unit_schedule_builds_target() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = UnitScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander],
            target: UnitKind::Engineer(TechLevel::T1),
            options: SearchOptions::default(),
            config: SchedulerConfig::default(),
        };

        let schedule = scheduler
            .schedule_unit(&request)
            .expect("schedule should succeed");
        assert!(!schedule.steps.is_empty(), "schedule should contain steps");
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
            config: SchedulerConfig::default(),
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
    fn greedy_eco_schedule_respects_max_mex_count() {
        let library = test_library();
        let scheduler = Scheduler::new(library);
        let request = EcoScheduleRequest {
            initial_eco: default_eco(),
            initial_inventory: vec![UnitKind::Commander, UnitKind::Mex(TechLevel::T1)],
            target: EcoTarget {
                mass_production: MassRate::from_raw(7.0),
                tolerance: 1.0,
            },
            options: SearchOptions::default(),
            config: SchedulerConfig { max_mex_count: 1 },
        };

        let schedule = scheduler
            .schedule_eco(&request)
            .expect("schedule should succeed");
        let new_mex_builds = schedule
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    &s.action,
                    Action::Build {
                        target: UnitKind::Mex(_) | UnitKind::CapMex(_),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            new_mex_builds, 0,
            "should not build new mass extractors when already at max_mex_count"
        );
        assert!(
            schedule.final_eco.production_per_second_mass >= 7.0,
            "final mass production should still meet target via upgrades"
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
            config: SchedulerConfig::default(),
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
