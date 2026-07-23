//! Shared types and conversions for FAF simulator consumers.
//!
//! This crate sits between `faf-sim` (the Bevy ECS runtime) and `faf-solver`
//! (the analytical solver), providing stable, serializable plan and economy
//! representations that can be used without pulling in Bevy.

pub mod economy_types;
pub mod plan_types;
pub mod scheduler_types;

pub use economy_types::{BuildQueue, BuildTask, EcoSnapshot, EconomyRuntimeState};
pub use plan_types::{ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary};
pub use scheduler_types::{
    Action, CandidateReasoning, CandidateScoreBreakdown, DirectionScores, PriorityTable, Schedule,
    ScheduleError, ScheduleWithReasoning, ScoreCategory, StepReasoning, StepResult,
};
