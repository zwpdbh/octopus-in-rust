//! Labeled samples used to train the build-time predictor.

use faf_sim::quantities::{Energy, Mass};
use faf_sim::runtime::{BuildQueue, BuildTask, EcoSnapshot};
use serde::{Deserialize, Serialize};

/// Number of scalar features fed into the model.
pub const FEATURE_DIM: usize = 17;

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
    /// Plan did not finish within the practical time limit.
    NotPractical,
}

impl EcoPlanLabel {
    /// Target value used for regression.
    ///
    /// Practical plans use `log(time)`; non-practical plans are clipped to the
    /// log of the practical time limit so the model learns to assign them the
    /// worst plausible score.
    pub fn regression_target(&self, time_limit_seconds: f64) -> f64 {
        match self {
            EcoPlanLabel::Practical { time_seconds } => time_seconds.ln(),
            EcoPlanLabel::NotPractical => time_limit_seconds.ln(),
        }
    }

    pub fn is_practical(&self) -> bool {
        matches!(self, EcoPlanLabel::Practical { .. })
    }
}

/// Aggregate statistics describing a plan, independent of the current economy.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanStats {
    pub total_mass_cost: f64,
    pub total_energy_cost: f64,
    pub total_build_time: f64,
    pub num_targets: usize,
    pub first_mass_cost: f64,
    pub first_energy_cost: f64,
    pub first_build_time: f64,
    pub total_production_mass: f64,
    pub total_production_energy: f64,
    pub total_maintenance_energy: f64,
    pub total_mass_storage: f64,
    pub total_energy_storage: f64,
    pub assigned_build_power: f64,
}

impl PlanStats {
    pub fn from_plan(plan: &[BuildTask]) -> Self {
        let mut stats = PlanStats::default();
        let mut first_target_seen = false;

        for task in plan {
            stats.assigned_build_power += task.builders.iter().map(|b| b.build_power).sum::<f64>();

            for target in &task.targets {
                stats.total_mass_cost += target.mass_cost;
                stats.total_energy_cost += target.energy_cost;
                stats.total_build_time += target.build_time;
                stats.total_production_mass += target.production_per_second_mass;
                stats.total_production_energy += target.production_per_second_energy;
                stats.total_maintenance_energy += target.maintenance_consumption_per_second_energy;
                stats.total_mass_storage += target.mass_storage;
                stats.total_energy_storage += target.energy_storage;
                stats.num_targets += 1;

                if !first_target_seen {
                    stats.first_mass_cost = target.mass_cost;
                    stats.first_energy_cost = target.energy_cost;
                    stats.first_build_time = target.build_time;
                    first_target_seen = true;
                }
            }
        }

        stats
    }
}

/// Extract a fixed-length feature vector from the initial economy and plan.
pub fn extract_features(initial_eco: &EcoSnapshot, plan: &[BuildTask]) -> [f64; FEATURE_DIM] {
    let plan = PlanStats::from_plan(plan);

    [
        initial_eco.production_per_second_mass,
        initial_eco.production_per_second_energy,
        initial_eco.maintenance_consumption_per_second_energy,
        initial_eco.mass_storage,
        initial_eco.energy_storage,
        initial_eco.mass_storage_cap,
        initial_eco.energy_storage_cap,
        plan.total_mass_cost,
        plan.total_energy_cost,
        plan.total_build_time,
        plan.num_targets as f64,
        plan.first_mass_cost,
        plan.first_energy_cost,
        plan.first_build_time,
        plan.total_production_mass,
        plan.total_production_energy,
        plan.assigned_build_power,
    ]
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
            current: Mass::from_raw(snapshot.mass_storage),
            cap: Mass::from_raw(snapshot.mass_storage_cap),
        },
        energy_storage: Storage {
            current: Energy::from_raw(snapshot.energy_storage),
            cap: Energy::from_raw(snapshot.energy_storage_cap),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_features_have_expected_length() {
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

        let features = extract_features(&snapshot, &[task]);
        assert_eq!(features.len(), FEATURE_DIM);
    }
}
