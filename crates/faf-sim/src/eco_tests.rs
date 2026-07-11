#[cfg(test)]
mod tests {
    use crate::economy::EconomyState;
    use crate::quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
    use crate::sim::{BuildQueue, BuildTask, Simulation, SimulationEvent, UnitDefRef};

    fn rich_eco() -> EconomyState {
        EconomyState {
            net_mass_income: MassRate::from_raw(1000.0),
            net_energy_income: EnergyRate::from_raw(1000.0),
            mass_storage: Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0)),
            energy_storage: Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0)),
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
            builders: vec![UnitDefRef {
                build_power: 10.0,
                mass_cost: 0.0,
                energy_cost: 0.0,
                build_time: 0.0,
                ..Default::default()
            }],
            target: UnitDefRef {
                build_power: 0.0,
                mass_cost: 100.0,
                energy_cost: 100.0,
                build_time: 100.0,
                ..Default::default()
            },
        }]);

        let mut sim = Simulation::new(queue, 1.0, 1000.0);
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
        assert_eq!(ticks, 10);
        assert!((sim.current_time() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn energy_stall_slows_build() {
        let mut eco = rich_eco();
        eco.net_energy_income = EnergyRate::from_raw(0.0);
        eco.energy_storage = Storage::new(Energy::from_raw(5.0), Energy::from_raw(10000.0));

        let queue = BuildQueue {
            initial_eco: eco,
            tasks: vec![BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitDefRef {
                    build_power: 10.0,
                    ..Default::default()
                }],
                target: UnitDefRef {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    ..Default::default()
                },
            }],
        };

        let mut sim = Simulation::new(queue, 1.0, 1000.0);
        let mut saw_stall = false;

        while !sim.is_finished() {
            for event in sim.step() {
                if let SimulationEvent::Ticked(s) = event {
                    if s.energy_stalled {
                        saw_stall = true;
                    }
                }
            }
        }

        assert!(saw_stall);
        // With only 5 energy available we cannot run at full power.
        assert!(sim.current_time() > 10.0);
    }
}
