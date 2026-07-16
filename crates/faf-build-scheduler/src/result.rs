//! Output types for the build scheduler.

use faf_sim::runtime::{BuildQueue, EcoSnapshot};
use faf_sim::units::UnitKind;

use crate::algorithms::AlgorithmKind;

/// A single step in the planned build order.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub action: Action,
    pub finish_time_seconds: f64,
    pub economy: EcoSnapshot,
}

/// A concrete action the scheduler decided to take.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Build {
        target: UnitKind,
        builder: UnitKind,
    },
    Upgrade {
        from: UnitKind,
        to: UnitKind,
        builder: UnitKind,
    },
}

/// The full planned schedule returned by a scheduling algorithm.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    pub build_queue: BuildQueue,
    pub total_time_seconds: f64,
    pub final_eco: EcoSnapshot,
    pub steps: Vec<StepResult>,
}

/// Errors that can occur during scheduling.
#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("algorithm {0:?} is not implemented yet")]
    AlgorithmNotImplemented(AlgorithmKind),
    #[error("no legal builder available for target {target:?}")]
    NoLegalBuilder { target: UnitKind },
    #[error("the requested goal is unreachable")]
    GoalUnreachable,
    #[error("the plan stalled during simulation")]
    SimulationStalled,
}
