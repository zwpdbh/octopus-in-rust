//! High-level scheduler facade.

use std::path::PathBuf;

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

    /// Create a scheduler using the default FAF units database shipped with the
    /// workspace.
    pub fn from_default_units(kind: AlgorithmKind) -> anyhow::Result<Self> {
        let path = default_units_path();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read units file {}: {e}", path.display()))?;
        let index: faf_units::DataIndex = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("failed to parse units file {}: {e}", path.display()))?;
        Ok(Self::with_algorithm(BlueprintLibrary::new(index), kind))
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

fn default_units_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/faf-units/data/faf_units.json")
}
