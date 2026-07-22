//! Bevy plugins and system sets that power the scheduler.

pub mod apply;
pub mod eco;
pub mod lifecycle;
pub mod unit;

pub use eco::EcoSchedulingPlugin;
pub use lifecycle::{
    run_to_completion, SchedulerLifecyclePlugin, SchedulerResult, SchedulerSet, SchedulerState,
};
pub use unit::UnitSchedulingPlugin;
