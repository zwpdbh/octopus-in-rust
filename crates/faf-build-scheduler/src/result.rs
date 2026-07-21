//! Output types for the build scheduler.
//!
//! These types live in [`faf_sim_shared::scheduler_types`] so that the backend and
//! frontend share a single serialized format. This module re-exports them for
//! convenience.

pub use faf_sim_shared::{Action, Schedule, ScheduleError, StepResult};
