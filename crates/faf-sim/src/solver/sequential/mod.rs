//! Fast, exact sequential-task completion-time solver.
//!
//! This module applies the same one-second economy map that the ECS simulator
//! uses, but without the Bevy runtime, event emission, or service overhead. It
//! is intentionally a direct tick loop: the FAF economy rules are a piecewise-
//! affine dynamical system that can enter limit cycles, so trying to jump many
//! ticks at once is error-prone. Advancing one second at a time is simple,
//! exact, and still much faster than the full simulation.

mod compute;
mod factor;
mod state;

#[cfg(test)]
mod tests;

pub use compute::{
    plan_completion_result, plan_completion_time, plan_completion_with_tasks,
    single_task_completion_result, single_task_completion_time, CompletionResult, PlanResult,
};
