//! Labeled samples used to train the build-time predictor.

use std::marker::PhantomData;

use faf_quantities::{StepTime, Time};
use faf_sim::sim::Simulation;
use faf_sim_shared::{BuildQueue, BuildTask, EcoSnapshot};
use serde::{Deserialize, Serialize};

/// Maximum number of tasks the sequence model accepts.
/// Plans with fewer tasks are zero-padded; longer plans are truncated.
pub const MAX_SEQ_LEN: usize = 10;

/// Number of scalar features describing a single task (including the `is_present` flag).
///
/// The vector contains the initial economy snapshot and task-level aggregates.
/// Cumulative contributions from earlier tasks are omitted because this version
/// of the predictor is trained on single-task plans only.
pub const TASK_FEATURE_DIM: usize = 22;

/// State marker for a sample that has not been simulated yet.
#[derive(Debug, Clone, Copy)]
pub struct Unsimulated;

/// State marker for a sample that has been simulated and carries a real label.
#[derive(Debug, Clone, Copy)]
pub struct Simulated;

/// Trait describing the label type associated with each sample state.
pub trait SampleState {
    type Label: serde::Serialize + for<'de> serde::Deserialize<'de>;
}

impl SampleState for Unsimulated {
    type Label = ();
}

impl SampleState for Simulated {
    type Label = f64;
}

/// A single training example: initial economy + plan, paired with the simulated
/// completion time.
///
/// The `State` type parameter enforces, at compile time, whether the sample has
/// been simulated:
///
/// - `EcoPlanSample<Unsimulated>` is produced by the generator. It has no label
///   and cannot be inserted into the training database.
/// - `EcoPlanSample<Simulated>` has a real completion time and is the only form
///   that can be serialized and inserted into the training database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "S::Label: serde::Serialize",
    deserialize = "S::Label: serde::Deserialize<'de>"
))]
pub struct EcoPlanSample<S: SampleState = Simulated> {
    pub initial_eco: EcoSnapshot,
    pub plan: Vec<BuildTask>,
    pub time_seconds: S::Label,
    #[serde(skip)]
    _state: PhantomData<S>,
}

impl EcoPlanSample<Unsimulated> {
    /// Create a new, un-simulated sample.
    pub fn new(initial_eco: EcoSnapshot, plan: Vec<BuildTask>) -> Self {
        Self {
            initial_eco,
            plan,
            time_seconds: (),
            _state: PhantomData,
        }
    }

    /// Produce a real completion time and transition to [`Simulated`].
    ///
    /// Single-task plans use the analytical solver, which is exact and much
    /// faster than the full ECS simulator. Multi-task plans still fall back to
    /// the simulator.
    pub fn simulate(self) -> EcoPlanSample<Simulated> {
        let time_seconds = if self.plan.len() == 1 {
            faf_solver::single_task_completion_time(
                &self.initial_eco,
                &self.plan[0],
                MAX_SIM_TIME_SECONDS,
            )
        } else {
            simulate_label(&self.initial_eco, &self.plan)
        };
        EcoPlanSample {
            initial_eco: self.initial_eco,
            plan: self.plan,
            time_seconds,
            _state: PhantomData,
        }
    }
}

impl EcoPlanSample<Simulated> {
    /// Simulated completion time.
    pub fn time_seconds(&self) -> f64 {
        self.time_seconds
    }

    /// Target value used for regression: `log(completion_time)`.
    pub fn regression_target(&self) -> f64 {
        self.time_seconds.ln()
    }
}

/// Aggregate statistics describing a task's builders and targets.
#[derive(Debug, Clone, Copy, Default)]
struct TaskStats {
    builder_count: usize,
    target_count: usize,
    build_power: f64,
    builder_maintenance: f64,
    mass_cost: f64,
    energy_cost: f64,
    build_time: f64,
    production_mass: f64,
    production_energy: f64,
    maintenance_energy: f64,
    mass_storage: f64,
    energy_storage: f64,
    first_mass_cost: f64,
    first_energy_cost: f64,
    first_build_time: f64,
}

impl TaskStats {
    pub fn from_task(task: &BuildTask) -> Self {
        let mut stats = TaskStats::default();
        stats.builder_count = task.builders.len();
        stats.target_count = task.targets.len();

        for builder in &task.builders {
            stats.build_power += builder.build_power;
            stats.builder_maintenance += builder.maintenance_consumption_per_second_energy;
        }

        let mut first_target_seen = false;
        for target in &task.targets {
            stats.mass_cost += target.mass_cost;
            stats.energy_cost += target.energy_cost;
            stats.build_time += target.build_time;
            stats.production_mass += target.production_per_second_mass;
            stats.production_energy += target.production_per_second_energy;
            stats.maintenance_energy += target.maintenance_consumption_per_second_energy;
            stats.mass_storage += target.mass_storage;
            stats.energy_storage += target.energy_storage;

            if !first_target_seen {
                stats.first_mass_cost = target.mass_cost;
                stats.first_energy_cost = target.energy_cost;
                stats.first_build_time = target.build_time;
                first_target_seen = true;
            }
        }

        stats
    }
}

/// Extract a fixed-length feature vector for a single task.
///
/// The vector includes:
/// - the initial economy snapshot the plan starts from,
/// - task-level aggregates (build power, costs, production, maintenance, storage).
pub(crate) fn extract_task_features(
    task: &BuildTask,
    initial_eco: &EcoSnapshot,
) -> [f64; TASK_FEATURE_DIM] {
    let t = TaskStats::from_task(task);

    let net_energy_start = initial_eco.production_per_second_energy.value()
        - initial_eco
            .maintenance_consumption_per_second_energy
            .value()
        - t.builder_maintenance;
    let first_build_time = t.first_build_time.max(1.0);
    let first_mass_drain = t.first_mass_cost / first_build_time * t.build_power;
    let first_energy_drain = t.first_energy_cost / first_build_time * t.build_power;

    [
        1.0, // is_present flag
        task.start_after.value(),
        initial_eco.production_per_second_mass.value(),
        initial_eco.production_per_second_energy.value(),
        initial_eco
            .maintenance_consumption_per_second_energy
            .value(),
        initial_eco.mass_storage.value(),
        initial_eco.energy_storage.value(),
        initial_eco.mass_storage_cap.value(),
        initial_eco.energy_storage_cap.value(),
        t.builder_count as f64,
        t.target_count as f64,
        t.build_power,
        t.builder_maintenance,
        t.mass_cost,
        t.energy_cost,
        t.build_time,
        t.production_mass,
        t.production_energy,
        t.maintenance_energy,
        net_energy_start,
        first_mass_drain,
        first_energy_drain,
    ]
}

/// Extract a sequence of per-task feature vectors from a plan.
///
/// For the single-task model each task is featurized independently; cumulative
/// contributions from earlier tasks are not included.
pub fn extract_sequence_features(
    initial_eco: &EcoSnapshot,
    plan: &[BuildTask],
) -> Vec<[f64; TASK_FEATURE_DIM]> {
    plan.iter()
        .map(|task| extract_task_features(task, initial_eco))
        .collect()
}

/// Build an initial economy from an `EcoSnapshot`.
///
/// The snapshot is the flat, serializable view used by the rest of the project,
/// so the generator works in terms of snapshots and only converts to the typed
/// runtime state when constructing a `BuildQueue`.
pub fn eco_snapshot_to_runtime_state(
    snapshot: &EcoSnapshot,
) -> faf_sim_shared::EconomyRuntimeState {
    use faf_quantities::Storage;
    use faf_sim_shared::EconomyRuntimeState;

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

/// Convenience helper to build a `BuildQueue` from a snapshot and a plan.
pub fn build_queue(snapshot: &EcoSnapshot, plan: Vec<BuildTask>) -> BuildQueue {
    BuildQueue {
        initial_eco: eco_snapshot_to_runtime_state(snapshot),
        tasks: plan,
    }
}

/// Hard upper bound on how long the simulator may run for a single sample.
///
/// This is a safety guard against pathological inputs; it is not a
/// practical/not-practical threshold.
const MAX_SIM_TIME_SECONDS: f64 = 6_000.0;

/// Run the simulator on a plan and return the completion time in seconds.
pub(crate) fn simulate_label(initial_eco: &EcoSnapshot, plan: &[BuildTask]) -> f64 {
    let dt = StepTime::from_seconds(1).expect("1 second dt is valid");
    let max_sim_time = Time::from_raw(MAX_SIM_TIME_SECONDS);
    let queue = build_queue(initial_eco, plan.to_vec());

    let mut sim = Simulation::new(queue, dt, Some(max_sim_time), None);

    while !sim.is_finished() {
        sim.step();
    }

    sim.current_time().value()
}

#[cfg(test)]
mod tests {
    use faf_blueprints::UnitEcoStats;
    use faf_quantities::{Energy, EnergyRate, Mass, MassRate};

    use super::*;

    #[test]
    fn extracted_sequence_features_have_expected_shape() {
        let snapshot = EcoSnapshot {
            time: Time::from_raw(0.0),
            production_per_second_mass: MassRate::from_raw(0.0),
            production_per_second_energy: EnergyRate::from_raw(0.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_drain: MassRate::from_raw(0.0),
            energy_drain: EnergyRate::from_raw(0.0),
            total_mass_spent: Mass::from_raw(0.0),
            total_energy_spent: Energy::from_raw(0.0),
            mass_storage: Mass::from_raw(0.0),
            mass_storage_cap: Mass::from_raw(0.0),
            energy_storage: Energy::from_raw(0.0),
            energy_storage_cap: Energy::from_raw(0.0),
        };
        let task = BuildTask {
            id: 0,
            start_after: Time::from_raw(1.0),
            builders: vec![UnitEcoStats {
                build_power: 10.0,
                ..Default::default()
            }],
            targets: vec![UnitEcoStats {
                mass_cost: 100.0,
                energy_cost: 500.0,
                build_time: 100.0,
                ..Default::default()
            }],
        };

        let features = extract_sequence_features(&snapshot, &[task]);
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].len(), TASK_FEATURE_DIM);
    }
}
