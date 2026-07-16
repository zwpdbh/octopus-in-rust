use super::state::EPS;
use super::*;
use crate::runtime::{BuildTask, EcoSnapshot, UnitEcoStats};
use crate::Time;

fn simple_task_with_start(
    power: f64,
    build_time: f64,
    mass_cost: f64,
    energy_cost: f64,
    start_after: f64,
) -> BuildTask {
    BuildTask {
        id: 0,
        start_after: Time::from_raw(start_after),
        builders: vec![UnitEcoStats {
            build_power: power,
            ..Default::default()
        }],
        targets: vec![UnitEcoStats {
            build_time,
            mass_cost,
            energy_cost,
            ..Default::default()
        }],
    }
}

fn simple_task(power: f64, build_time: f64, mass_cost: f64, energy_cost: f64) -> BuildTask {
    simple_task_with_start(power, build_time, mass_cost, energy_cost, 1.0)
}

fn eco(
    mass_income: f64,
    energy_income: f64,
    maintenance: f64,
    mass_storage: f64,
    energy_storage: f64,
) -> EcoSnapshot {
    EcoSnapshot {
        time: 0.0,
        production_per_second_mass: mass_income,
        production_per_second_energy: energy_income,
        maintenance_consumption_per_second_energy: maintenance,
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage,
        mass_storage_cap: 1000.0,
        energy_storage,
        energy_storage_cap: 1000.0,
    }
}

#[test]
fn no_stall_completes_in_build_time_over_power() {
    let t = simple_task(10.0, 100.0, 100.0, 1000.0);
    let e = eco(10.0, 100.0, 0.0, 1000.0, 1000.0);
    assert!((single_task_completion_time(&e, &t, 6000.0) - 11.0).abs() < EPS);
}

#[test]
fn energy_stall_steady_state() {
    let t = simple_task_with_start(10.0, 100.0, 100.0, 2000.0, 0.0);
    let e = eco(10.0, 50.0, 0.0, 1000.0, 0.0);
    assert!((single_task_completion_time(&e, &t, 6000.0) - 40.0).abs() < EPS);
}

#[test]
fn mass_stall_steady_state() {
    let t = simple_task_with_start(10.0, 100.0, 2000.0, 100.0, 0.0);
    let e = eco(50.0, 100.0, 0.0, 0.0, 1000.0);
    assert!((single_task_completion_time(&e, &t, 6000.0) - 40.0).abs() < EPS);
}

#[test]
fn regression_off_by_one_case() {
    use crate::economy::EconomyRuntimeState;
    use crate::quantities::{
        Energy, EnergyRate, Mass, MassRate, StepTime, Storage, Time as SimTime,
    };
    use crate::runtime::BuildQueue;
    use crate::sim::Simulation;

    let task = BuildTask {
        id: 0,
        start_after: Time::from_raw(1.0),
        builders: vec![UnitEcoStats {
            build_power: 19.2,
            maintenance_consumption_per_second_energy: 100.0,
            ..Default::default()
        }],
        targets: vec![
            UnitEcoStats {
                mass_cost: 270.0,
                energy_cost: 8000.0,
                build_time: 1600.0,
                ..Default::default()
            },
            UnitEcoStats {
                mass_cost: 1280.0,
                energy_cost: 14000.0,
                build_time: 4800.0,
                ..Default::default()
            },
            UnitEcoStats {
                mass_cost: 360.0,
                energy_cost: 2880.0,
                build_time: 1440.0,
                ..Default::default()
            },
        ],
    };
    let snapshot = EcoSnapshot {
        time: 0.0,
        production_per_second_mass: 9.0,
        production_per_second_energy: 200.0,
        maintenance_consumption_per_second_energy: 8.0,
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage: 650.0,
        mass_storage_cap: 650.0,
        energy_storage: 3900.0,
        energy_storage_cap: 3900.0,
    };
    let solver_time = single_task_completion_time(&snapshot, &task, 6000.0);

    let queue = BuildQueue {
        initial_eco: EconomyRuntimeState {
            production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
            production_per_second_energy: EnergyRate::from_raw(
                snapshot.production_per_second_energy,
            ),
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
        },
        tasks: vec![task],
    };
    let dt = StepTime::from_seconds(1).unwrap();
    let mut sim = Simulation::new(queue, dt, Some(SimTime::from_raw(6000.0)), None);
    while !sim.is_finished() {
        sim.step();
    }
    let sim_time = sim.current_time().value();
    assert!((solver_time - sim_time).abs() < EPS);
}

#[test]
fn plan_solver_matches_simulator_for_two_task_sequence() {
    use crate::economy::EconomyRuntimeState;
    use crate::quantities::{
        Energy, EnergyRate, Mass, MassRate, StepTime, Storage, Time as SimTime,
    };
    use crate::runtime::BuildQueue;
    use crate::sim::Simulation;

    let tasks = vec![
        BuildTask {
            id: 1,
            start_after: Time::from_raw(1.0),
            builders: vec![UnitEcoStats {
                build_power: 10.0,
                ..Default::default()
            }],
            targets: vec![UnitEcoStats {
                build_time: 100.0,
                mass_cost: 100.0,
                energy_cost: 1000.0,
                production_per_second_energy: 50.0,
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
                build_time: 100.0,
                mass_cost: 200.0,
                energy_cost: 2000.0,
                ..Default::default()
            }],
        },
    ];
    let snapshot = EcoSnapshot {
        time: 0.0,
        production_per_second_mass: 10.0,
        production_per_second_energy: 10.0,
        maintenance_consumption_per_second_energy: 0.0,
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage: 1000.0,
        mass_storage_cap: 1000.0,
        energy_storage: 5000.0,
        energy_storage_cap: 5000.0,
    };

    let solver_time = plan_completion_time(&snapshot, &tasks, 6000.0);

    let queue = BuildQueue {
        initial_eco: EconomyRuntimeState {
            production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
            production_per_second_energy: EnergyRate::from_raw(
                snapshot.production_per_second_energy,
            ),
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
        },
        tasks,
    };
    let dt = StepTime::from_seconds(1).unwrap();
    let mut sim = Simulation::new(queue, dt, Some(SimTime::from_raw(6000.0)), None);
    while !sim.is_finished() {
        sim.step();
    }
    let sim_time = sim.current_time().value();

    assert!(
        (solver_time - sim_time).abs() < EPS,
        "solver {} != sim {} for two-task plan",
        solver_time,
        sim_time
    );
}

#[test]
fn matches_simulator_for_simple_cases() {
    use crate::economy::EconomyRuntimeState;
    use crate::quantities::{
        Energy, EnergyRate, Mass, MassRate, StepTime, Storage, Time as SimTime,
    };
    use crate::runtime::BuildQueue;
    use crate::sim::Simulation;

    let cases = [
        (
            simple_task(10.0, 100.0, 100.0, 1000.0),
            eco(10.0, 100.0, 0.0, 1000.0, 1000.0),
        ),
        (
            simple_task_with_start(10.0, 100.0, 100.0, 2000.0, 0.0),
            eco(10.0, 50.0, 0.0, 1000.0, 0.0),
        ),
        (
            simple_task_with_start(10.0, 100.0, 2000.0, 100.0, 0.0),
            eco(50.0, 100.0, 0.0, 0.0, 1000.0),
        ),
    ];

    for (task, snapshot) in cases {
        let solver_time = single_task_completion_time(&snapshot, &task, 6000.0);

        let queue = BuildQueue {
            initial_eco: EconomyRuntimeState {
                production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
                production_per_second_energy: EnergyRate::from_raw(
                    snapshot.production_per_second_energy,
                ),
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
            },
            tasks: vec![task],
        };
        let dt = StepTime::from_seconds(1).unwrap();
        let mut sim = Simulation::new(queue, dt, Some(SimTime::from_raw(6000.0)), None);
        while !sim.is_finished() {
            sim.step();
        }
        let sim_time = sim.current_time().value();

        assert!(
            (solver_time - sim_time).abs() < EPS,
            "solver {} != sim {} for snapshot {:?}",
            solver_time,
            sim_time,
            snapshot
        );
    }
}
