//! Greedy scheduling algorithm placeholder.

use faf_sim::units::BlueprintLibrary;

use crate::algorithms::SchedulingAlgorithm;
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError};

/// Greedy best-first scheduler.
///
/// The implementation is intentionally left as `todo!()` while the design of
/// the heuristic and state representation is finalized.
#[derive(Debug, Default)]
pub struct Greedy;

impl Greedy {
    /// Create a new greedy scheduler placeholder.
    pub fn new() -> Self {
        Self
    }
}

impl SchedulingAlgorithm for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn schedule_eco(
        &self,
        _library: &BlueprintLibrary,
        _request: &EcoScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        todo!("implement greedy eco scheduling")
    }

    fn schedule_unit(
        &self,
        _library: &BlueprintLibrary,
        _request: &UnitScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        todo!("implement greedy unit scheduling")
    }
}
