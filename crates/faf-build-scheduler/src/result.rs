//! Output types for the build scheduler.

use faf_blueprints::UnitKind;
use faf_sim_shared::ConstructionPlan;
use faf_sim_shared::{BuildQueue, EcoSnapshot};
use serde::{Deserialize, Serialize};

use crate::algorithms::AlgorithmKind;

/// A single step in the planned build order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub action: Action,
    pub finish_time_seconds: f64,
    pub economy: EcoSnapshot,
}

/// A concrete action the scheduler decided to take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Build { target: UnitKind, builder: UnitKind },
    Upgrade { from: UnitKind, to: UnitKind },
}

/// The full planned schedule returned by a scheduling algorithm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    /// The solved plan in the shared, human-editable format.
    pub plan: ConstructionPlan,
    pub total_time_seconds: f64,
    pub final_eco: EcoSnapshot,
    pub steps: Vec<StepResult>,
}

impl Schedule {
    /// Convert the solved plan into the simulator's runtime queue format.
    pub fn to_build_queue(&self) -> BuildQueue {
        self.plan.to_build_queue()
    }
}

/// Errors that can occur during scheduling.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ScheduleError {
    #[error("algorithm {0:?} is not implemented yet")]
    AlgorithmNotImplemented(AlgorithmKind),
    #[error("no legal builder available for target {target:?}")]
    NoLegalBuilder { target: UnitKind },
    #[error("the requested goal is unreachable")]
    GoalUnreachable,
    #[error("the plan stalled during simulation")]
    SimulationStalled,
    #[error("the search timed out")]
    SearchTimeout,
}
