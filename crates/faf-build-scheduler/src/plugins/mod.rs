//! Bevy plugins and system sets that power the scheduler.

pub mod apply;
pub mod eco;
pub mod events;
pub mod lifecycle;
pub mod unit;

pub use eco::EcoSchedulingPlugin;
pub use events::SchedulerStepEvent;
pub use lifecycle::{
    run_to_completion, run_to_completion_best_effort, run_to_completion_cancellable,
    run_to_completion_with_reasoning_cancellable, SchedulerLifecyclePlugin, SchedulerResult,
    SchedulerSet, SchedulerState,
};
pub use unit::UnitSchedulingPlugin;
