//! Bevy plugins and system sets that power the scheduler.

pub mod eco;
pub mod greedy;
pub mod init;
pub mod unit;

pub use eco::EcoSchedulingPlugin;
pub use greedy::GreedyPlugin;
pub use init::{
    run_to_completion, SchedulerInitPlugin, SchedulerResult, SchedulerSet, SchedulerState,
};
pub use unit::UnitSchedulingPlugin;
