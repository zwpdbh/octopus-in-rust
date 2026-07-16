//! Analytical solvers that mirror the discrete simulation rules.
//!
//! These are intended as fast alternatives to stepping the ECS runtime when the
//! input shape is restricted enough to make closed-form or stage-based
//! computation practical.

mod sequential;

pub use sequential::{
    plan_completion_result, plan_completion_time, plan_completion_with_tasks,
    single_task_completion_result, single_task_completion_time, CompletionResult, PlanResult,
};
