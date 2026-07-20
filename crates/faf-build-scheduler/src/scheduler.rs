//! High-level scheduler facade.

use std::sync::Arc;

use crate::algorithms::{algorithm_by_kind, AlgorithmKind, SchedulingAlgorithm};
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError};
use faf_blueprints::BlueprintLibrary;

/// Build-order scheduler.
///
/// Holds a `BlueprintLibrary` and dispatches requests to a selectable
/// [`SchedulingAlgorithm`].
pub struct Scheduler {
    library: Arc<BlueprintLibrary>,
    algorithm: Box<dyn SchedulingAlgorithm>,
}

impl Scheduler {
    /// Create a scheduler with the given algorithm.
    pub fn with_algorithm(library: BlueprintLibrary, kind: AlgorithmKind) -> Self {
        Self {
            library: Arc::new(library),
            algorithm: algorithm_by_kind(kind),
        }
    }

    /// Create a scheduler using the default greedy algorithm.
    pub fn new(library: BlueprintLibrary) -> Self {
        Self::with_algorithm(library, AlgorithmKind::Greedy)
    }

    /// Create a scheduler using the default FAF units database shipped with the
    /// workspace.
    pub fn from_default_units(kind: AlgorithmKind) -> anyhow::Result<Self> {
        let library = BlueprintLibrary::from_default_units()?;
        Ok(Self::with_algorithm(library, kind))
    }

    /// Plan the fastest way to reach the eco target.
    pub fn schedule_eco(&self, request: &EcoScheduleRequest) -> Result<Schedule, ScheduleError> {
        self.algorithm
            .schedule_eco(Arc::clone(&self.library), request)
    }

    /// Plan the fastest way to build the target unit.
    pub fn schedule_unit(&self, request: &UnitScheduleRequest) -> Result<Schedule, ScheduleError> {
        self.algorithm
            .schedule_unit(Arc::clone(&self.library), request)
    }
}
