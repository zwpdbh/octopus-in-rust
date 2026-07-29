//! Build-order scheduler for FAF.
//!
//! This crate exposes a pluggable scheduling abstraction over the `faf-sim`
//! blueprint library and solver. The default implementation is a placeholder
//! that proves the wiring; real search algorithms can be added behind the same
//! [`SchedulingAlgorithm`] trait.

pub mod algorithms;
pub mod app;
pub mod components;
pub mod config;
pub mod plugins;
pub mod request;
pub mod resources;
pub mod result;
pub mod scheduler;
pub mod search;
pub mod util;

pub use algorithms::{algorithm_by_kind, AlgorithmKind, Greedy, SchedulingAlgorithm};
pub use config::SchedulerConfig;
pub use plugins::{
    eco::decide_direction::{DirectionScores, PriorityTable},
    eco::observe::{EngineerCounts, FactoryTier, Observation},
    run_to_completion, run_to_completion_cancellable, run_to_completion_with_reasoning_cancellable,
    EcoSchedulingPlugin, SchedulerLifecyclePlugin, SchedulerResult, SchedulerSet, SchedulerState,
    SchedulerStepEvent, UnitSchedulingPlugin,
};
pub use request::{
    EcoScheduleInput, EcoScheduleRequest, EcoTarget, SearchOptions, UnitScheduleInput,
    UnitScheduleRequest,
};
pub use resources::{SchedulerClock, SearchGoal, SearchProgress, StepLog, TaskLog};
pub use result::{
    Action, CandidateReasoning, Schedule, ScheduleError, ScheduleWithReasoning, StepReasoning,
    StepResult,
};
pub use scheduler::{ScheduleStreamEvent, Scheduler};
