//! Scheduling algorithm abstraction and registry.

use clap::ValueEnum;

use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError};
use faf_sim::units::BlueprintLibrary;

mod placeholder;
pub use placeholder::Placeholder;

/// Selectable scheduling algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AlgorithmKind {
    /// Wiring-only algorithm that returns a trivial valid schedule.
    Placeholder,
    /// Greedy best-first search (not implemented yet).
    Greedy,
    /// Beam search (not implemented yet).
    BeamSearch,
}

/// A scheduling algorithm that can plan eco or unit targets.
pub trait SchedulingAlgorithm: Send + Sync {
    fn name(&self) -> &'static str;

    fn schedule_eco(
        &self,
        library: &BlueprintLibrary,
        request: &EcoScheduleRequest,
    ) -> Result<Schedule, ScheduleError>;

    fn schedule_unit(
        &self,
        library: &BlueprintLibrary,
        request: &UnitScheduleRequest,
    ) -> Result<Schedule, ScheduleError>;
}

/// Instantiate the algorithm identified by `kind`.
pub fn algorithm_by_kind(kind: AlgorithmKind) -> Box<dyn SchedulingAlgorithm> {
    match kind {
        AlgorithmKind::Placeholder => Box::new(Placeholder::new()),
        AlgorithmKind::Greedy | AlgorithmKind::BeamSearch => Box::new(Unimplemented::new(kind)),
    }
}

/// Stub algorithm that reports which algorithm the caller asked for.
struct Unimplemented {
    kind: AlgorithmKind,
}

impl Unimplemented {
    fn new(kind: AlgorithmKind) -> Self {
        Self { kind }
    }
}

impl SchedulingAlgorithm for Unimplemented {
    fn name(&self) -> &'static str {
        "unimplemented"
    }

    fn schedule_eco(
        &self,
        _library: &BlueprintLibrary,
        _request: &EcoScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        Err(ScheduleError::AlgorithmNotImplemented(self.kind))
    }

    fn schedule_unit(
        &self,
        _library: &BlueprintLibrary,
        _request: &UnitScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        Err(ScheduleError::AlgorithmNotImplemented(self.kind))
    }
}
