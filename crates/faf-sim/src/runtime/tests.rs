#[cfg(test)]
mod tests {
    use crate::economy::EconomyRuntimeState;
    use crate::quantities::{Energy, EnergyRate, Mass, MassRate, StepTime, Storage, Time};
    use crate::runtime::{BuildQueue, BuildTask, SimulationEvent, UnitEcoStats};
    use crate::sim::Simulation;

    fn rich_eco() -> EconomyRuntimeState {
        EconomyRuntimeState {
            production_per_second_mass: MassRate::from_raw(1000.0),
            production_per_second_energy: EnergyRate::from_raw(1000.0),
            mass_storage: Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0)),
            energy_storage: Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0)),
            ..Default::default()
        }
    }

    fn make_queue(tasks: Vec<BuildTask>) -> BuildQueue {
        BuildQueue {
            initial_eco: rich_eco(),
            tasks,
        }
    }

    #[test]
    fn single_task_runs_and_completes() {
        let queue = make_queue(vec![BuildTask {
            id: 1,
            start_after: Time::from_raw(0.0),
            builders: vec![UnitEcoStats {
                build_power: 10.0,
                mass_cost: 0.0,
                energy_cost: 0.0,
                build_time: 0.0,
                ..Default::default()
            }],
            targets: vec![UnitEcoStats {
                build_power: 0.0,
                mass_cost: 100.0,
                energy_cost: 100.0,
                build_time: 100.0,
                ..Default::default()
            }],
        }]);

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(30.0),
        );
        let mut started = false;
        let mut completed = false;
        let mut ticks = 0;

        while !sim.is_finished() {
            for event in sim.step() {
                match event {
                    SimulationEvent::TaskStarted { task_id, .. } => {
                        assert_eq!(*task_id, 1);
                        started = true;
                    }
                    SimulationEvent::TaskCompleted { task_id, .. } => {
                        assert_eq!(*task_id, 1);
                        completed = true;
                    }
                    SimulationEvent::Ticked(_) => ticks += 1,
                    SimulationEvent::Finished => {}
                }
            }
        }

        assert!(started);
        assert!(completed);
        // 10 ticks for the build itself plus the 30-second post-queue tail.
        assert_eq!(ticks, 40);
        assert!((sim.current_time().value() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn second_task_starts_after_first_finishes() {
        let queue = make_queue(vec![
            BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitEcoStats {
                    build_power: 10.0,
                    ..Default::default()
                }],
                targets: vec![UnitEcoStats {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    ..Default::default()
                }],
            },
            BuildTask {
                id: 2,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitEcoStats {
                    build_power: 10.0,
                    ..Default::default()
                }],
                targets: vec![UnitEcoStats {
                    build_power: 0.0,
                    mass_cost: 50.0,
                    energy_cost: 50.0,
                    build_time: 50.0,
                    ..Default::default()
                }],
            },
        ]);

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(30.0),
        );

        let mut task1_complete: Option<f64> = None;
        let mut task2_start: Option<f64> = None;
        while !sim.is_finished() {
            for event in sim.step() {
                match event {
                    SimulationEvent::TaskCompleted { task_id, time } if *task_id == 1 => {
                        task1_complete = Some(*time);
                    }
                    SimulationEvent::TaskStarted { task_id, time } if *task_id == 2 => {
                        task2_start = Some(*time);
                    }
                    _ => {}
                }
            }
        }

        assert!(task1_complete.is_some(), "task 1 should complete");
        assert!(task2_start.is_some(), "task 2 should start");
        assert!(
            task2_start.unwrap() >= task1_complete.unwrap(),
            "task 2 must start at or after task 1 finishes (got start={task2_start:?}, complete={task1_complete:?})"
        );
    }

    #[test]
    fn energy_stall_slows_build() {
        let mut eco = rich_eco();
        eco.production_per_second_energy = EnergyRate::from_raw(0.0);
        eco.energy_storage = Storage::new(Energy::from_raw(5.0), Energy::from_raw(10000.0));

        let queue = BuildQueue {
            initial_eco: eco,
            tasks: vec![BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitEcoStats {
                    build_power: 10.0,
                    ..Default::default()
                }],
                targets: vec![UnitEcoStats {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    ..Default::default()
                }],
            }],
        };

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(30.0),
        );

        while !sim.is_finished() {
            let _ = sim.step();
        }

        // With only 5 energy available we cannot run at full power.
        assert!(sim.current_time() > Time::from_raw(10.0));
    }

    #[test]
    fn faf_energy_stall_scales_mass_income_through_maintenance() {
        // A mex-like initial producer: 2 mass/s income, 0 gross energy production,
        // 2 energy/s maintenance. With empty energy storage, FAF scales mass income
        // by the army energy efficiency (0 / 2 = 0), so mass income drops to zero
        // even though there is no construction.
        let mut eco = rich_eco();
        eco.production_per_second_mass = MassRate::from_raw(2.0);
        eco.production_per_second_energy = EnergyRate::from_raw(0.0);
        eco.maintenance_consumption_per_second_energy = EnergyRate::from_raw(2.0);
        eco.mass_storage = Storage::new(Mass::from_raw(0.0), Mass::from_raw(10000.0));
        eco.energy_storage = Storage::new(Energy::from_raw(0.0), Energy::from_raw(10000.0));

        let queue = BuildQueue {
            initial_eco: eco,
            tasks: vec![],
        };

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(30.0),
        );

        let mut saw_scaled_mass_income = false;
        while !sim.is_finished() {
            for event in sim.step() {
                if let SimulationEvent::Ticked(s) = event {
                    if s.maintenance_consumption_per_second_energy.value() > 1.0
                        && s.mass_storage.value().abs() < 1e-9
                    {
                        saw_scaled_mass_income = true;
                    }
                }
            }
        }

        assert!(
            saw_scaled_mass_income,
            "`ProductionPerSecondMass` should be scaled to zero by FAF energy efficiency"
        );
    }

    #[test]
    fn mass_storage_adjacency_boosts_production() {
        use crate::economy::EconomyRuntimeState;
        use crate::runtime::AdjacencyBonus;

        // A mex with 4 adjacent mass storages produces 1.5x its base mass income.
        let base_mass_income = 6.0;
        let mut initial_eco = EconomyRuntimeState::default();
        initial_eco.production_per_second_energy = EnergyRate::from_raw(1000.0);
        initial_eco.mass_storage = Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0));
        initial_eco.energy_storage =
            Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0));
        let queue = BuildQueue {
            initial_eco,
            tasks: vec![BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitEcoStats {
                    build_power: 10.0,
                    ..Default::default()
                }],
                targets: vec![UnitEcoStats {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    production_per_second_mass: base_mass_income,
                    adjacency: AdjacencyBonus {
                        mass_storage_sides: 4,
                        ..Default::default()
                    },
                    ..Default::default()
                }],
            }],
        };

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(5.0),
        );

        let mut saw_boosted_income = false;
        while !sim.is_finished() {
            for event in sim.step() {
                if let SimulationEvent::Ticked(s) = event {
                    // The target finishes at time 10.0; check post-completion ticks.
                    if s.time.value() > 10.0
                        && (s.production_per_second_mass.value() - base_mass_income * 1.5).abs()
                            < 1e-9
                    {
                        saw_boosted_income = true;
                    }
                }
            }
        }

        assert!(
            saw_boosted_income,
            "mass storage adjacency should boost mex production by 50%"
        );
    }

    #[test]
    fn partial_mass_storage_adjacency_scales_linearly() {
        use crate::economy::EconomyRuntimeState;
        use crate::runtime::AdjacencyBonus;

        // A mex with 2 adjacent mass storages produces 1.25x its base mass income.
        let base_mass_income = 6.0;
        let mut initial_eco = EconomyRuntimeState::default();
        initial_eco.production_per_second_energy = EnergyRate::from_raw(1000.0);
        initial_eco.mass_storage = Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0));
        initial_eco.energy_storage =
            Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0));
        let queue = BuildQueue {
            initial_eco,
            tasks: vec![BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitEcoStats {
                    build_power: 10.0,
                    ..Default::default()
                }],
                targets: vec![UnitEcoStats {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    production_per_second_mass: base_mass_income,
                    adjacency: AdjacencyBonus {
                        mass_storage_sides: 2,
                        ..Default::default()
                    },
                    ..Default::default()
                }],
            }],
        };

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(5.0),
        );

        let mut saw_boosted_income = false;
        while !sim.is_finished() {
            for event in sim.step() {
                if let SimulationEvent::Ticked(s) = event {
                    // The target finishes at time 10.0; check post-completion ticks.
                    if s.time.value() > 10.0
                        && (s.production_per_second_mass.value() - base_mass_income * 1.25).abs()
                            < 1e-9
                    {
                        saw_boosted_income = true;
                    }
                }
            }
        }

        assert!(
            saw_boosted_income,
            "two mass storages should boost mex production by 25%"
        );
    }

    #[test]
    fn step_with_dt_advances_by_requested_amount_and_restores_default_dt() {
        let queue = make_queue(vec![BuildTask {
            id: 1,
            start_after: Time::from_raw(0.0),
            builders: vec![UnitEcoStats {
                build_power: 10.0,
                ..Default::default()
            }],
            targets: vec![UnitEcoStats {
                build_power: 0.0,
                mass_cost: 100.0,
                energy_cost: 100.0,
                build_time: 100.0,
                ..Default::default()
            }],
        }]);

        let mut sim = Simulation::new(
            queue,
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            Some(30.0),
        );

        // Take one manual step of 2.0 seconds.
        let events = sim.step_with_dt(StepTime::from_seconds(2).unwrap());
        assert!(events
            .iter()
            .any(|e| matches!(e, SimulationEvent::Ticked(_))));
        assert!((sim.current_time().value() - 2.0).abs() < 1e-9);

        // A subsequent normal step should use the configured dt of 1.0.
        sim.step();
        assert!((sim.current_time().value() - 3.0).abs() < 1e-9);
    }
}
