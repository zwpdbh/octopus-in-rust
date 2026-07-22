//! Bevy plugins and system sets that power the scheduler.

pub mod apply;
pub mod eco;
pub mod lifecycle;
pub mod trace;
pub mod unit;

pub use eco::EcoSchedulingPlugin;
pub use lifecycle::{
    run_to_completion, run_to_completion_best_effort, SchedulerLifecyclePlugin, SchedulerResult,
    SchedulerSet, SchedulerState,
};
pub use trace::SchedulerTracePlugin;
pub use unit::UnitSchedulingPlugin;
