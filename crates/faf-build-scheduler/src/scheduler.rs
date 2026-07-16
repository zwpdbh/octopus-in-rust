//! High-level scheduler facade.

use crate::algorithms::{algorithm_by_kind, AlgorithmKind, SchedulingAlgorithm};
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError};
use faf_sim::units::BlueprintLibrary;

/// Build-order scheduler.
///
/// Holds a `BlueprintLibrary` and dispatches requests to a selectable
/// [`SchedulingAlgorithm`].
pub struct Scheduler {
    library: BlueprintLibrary,
    algorithm: Box<dyn SchedulingAlgorithm>,
}

impl Scheduler {
    /// Create a scheduler with the given algorithm.
    pub fn with_algorithm(library: BlueprintLibrary, kind: AlgorithmKind) -> Self {
        Self {
            library,
            algorithm: algorithm_by_kind(kind),
        }
    }

    /// Create a scheduler using the default placeholder algorithm.
    pub fn new(library: BlueprintLibrary) -> Self {
        Self::with_algorithm(library, AlgorithmKind::Placeholder)
    }

    /// Plan the fastest way to reach the eco target.
    pub fn schedule_eco(&self, request: &EcoScheduleRequest) -> Result<Schedule, ScheduleError> {
        self.algorithm.schedule_eco(&self.library, request)
    }

    /// Plan the fastest way to build the target unit.
    pub fn schedule_unit(&self, request: &UnitScheduleRequest) -> Result<Schedule, ScheduleError> {
        self.algorithm.schedule_unit(&self.library, request)
    }
}
