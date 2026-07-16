//! Build-order scheduler for FAF.
//!
//! This crate exposes a pluggable scheduling abstraction over the `faf-sim`
//! blueprint library and solver. The default implementation is a placeholder
//! that proves the wiring; real search algorithms can be added behind the same
//! [`SchedulingAlgorithm`] trait.

pub mod algorithms;
pub mod request;
pub mod result;
pub mod scheduler;
pub mod util;

pub use algorithms::{algorithm_by_kind, AlgorithmKind, Placeholder, SchedulingAlgorithm};
pub use request::{EcoScheduleRequest, EcoTarget, SearchOptions, UnitScheduleRequest};
pub use result::{Action, Schedule, ScheduleError, StepResult};
pub use scheduler::Scheduler;
