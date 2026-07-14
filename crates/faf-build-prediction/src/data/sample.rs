//! Labeled samples used to train the build-time predictor.

use faf_sim::runtime::{BuildTask, EcoSnapshot};
use serde::{Deserialize, Serialize};

/// Maximum number of tasks the sequence model accepts.
/// Plans with fewer tasks are zero-padded; longer plans are truncated.
pub const MAX_SEQ_LEN: usize = 10;

/// Number of scalar features describing a single task (including the `is_present` flag).
pub const TASK_FEATURE_DIM: usize = 22;

/// A single training example: initial economy + plan, paired with the simulated
/// completion time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcoPlanSample {
    pub initial_eco: EcoSnapshot,
    pub plan: Vec<BuildTask>,
    pub label: EcoPlanLabel,
}

/// The supervised target for a sample.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EcoPlanLabel {
    /// Plan finished within the practical time limit.
    Practical { time_seconds: f64 },
    /// Plan did not finish within the practical time limit, but the simulator
    /// ran until a larger cap so the model can learn how slow it is.
    NotPractical { time_seconds: f64 },
}

impl EcoPlanLabel {
    /// Target value used for regression: `log(completion_time_or_cap)`.
    pub fn regression_target(&self) -> f64 {
        let time_seconds = match self {
            EcoPlanLabel::Practical { time_seconds } => *time_seconds,
            EcoPlanLabel::NotPractical { time_seconds } => *time_seconds,
        };
        time_seconds.ln()
    }

    pub fn is_practical(&self) -> bool {
        matches!(self, EcoPlanLabel::Practical { .. })
    }

    pub fn time_seconds(&self) -> f64 {
        match self {
            EcoPlanLabel::Practical { time_seconds } => *time_seconds,
            EcoPlanLabel::NotPractical { time_seconds } => *time_seconds,
        }
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
/// The vector includes the initial economy snapshot so the model can relate
/// each task to the economy it starts from, plus task-level aggregates that
/// encode build power, costs, production, maintenance, and storage.
pub fn extract_task_features(
    task: &BuildTask,
    initial_eco: &EcoSnapshot,
) -> [f64; TASK_FEATURE_DIM] {
    let t = TaskStats::from_task(task);

    let net_energy_start = initial_eco.production_per_second_energy
        - initial_eco.maintenance_consumption_per_second_energy
        - t.builder_maintenance;
    let first_build_time = t.first_build_time.max(1.0);
    let first_mass_drain = t.first_mass_cost / first_build_time * t.build_power;
    let first_energy_drain = t.first_energy_cost / first_build_time * t.build_power;

    [
        1.0, // is_present flag
        task.start_after.value(),
        initial_eco.production_per_second_mass,
        initial_eco.production_per_second_energy,
        initial_eco.maintenance_consumption_per_second_energy,
        initial_eco.mass_storage,
        initial_eco.energy_storage,
        initial_eco.mass_storage_cap,
        initial_eco.energy_storage_cap,
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
) -> faf_sim::economy::EconomyRuntimeState {
    use faf_sim::economy::EconomyRuntimeState;
    use faf_sim::quantities::{EnergyRate, MassRate, Storage};

    EconomyRuntimeState {
        production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
        production_per_second_energy: EnergyRate::from_raw(snapshot.production_per_second_energy),
        maintenance_consumption_per_second_energy: EnergyRate::from_raw(
            snapshot.maintenance_consumption_per_second_energy,
        ),
        mass_storage: Storage {
            current: faf_sim::quantities::Mass::from_raw(snapshot.mass_storage),
            cap: faf_sim::quantities::Mass::from_raw(snapshot.mass_storage_cap),
        },
        energy_storage: Storage {
            current: faf_sim::quantities::Energy::from_raw(snapshot.energy_storage),
            cap: faf_sim::quantities::Energy::from_raw(snapshot.energy_storage_cap),
        },
    }
}

/// Convenience helper to build a `BuildQueue` from a snapshot and a plan.
pub fn build_queue(snapshot: &EcoSnapshot, plan: Vec<BuildTask>) -> faf_sim::runtime::BuildQueue {
    faf_sim::runtime::BuildQueue {
        initial_eco: eco_snapshot_to_runtime_state(snapshot),
        tasks: plan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_sequence_features_have_expected_shape() {
        let snapshot = EcoSnapshot {
            time: 0.0,
            production_per_second_mass: 0.0,
            production_per_second_energy: 0.0,
            maintenance_consumption_per_second_energy: 0.0,
            mass_drain: 0.0,
            energy_drain: 0.0,
            total_mass_spent: 0.0,
            total_energy_spent: 0.0,
            mass_storage: 0.0,
            mass_storage_cap: 0.0,
            energy_storage: 0.0,
            energy_storage_cap: 0.0,
        };
        let task = BuildTask {
            id: 0,
            start_after: faf_sim::quantities::Time::from_raw(1.0),
            builders: vec![faf_sim::runtime::UnitEcoStats {
                build_power: 10.0,
                ..Default::default()
            }],
            targets: vec![faf_sim::runtime::UnitEcoStats {
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
