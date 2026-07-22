//! Lifecycle events emitted by the scheduler.
//!
//! These events are fired by the scheduling pipeline so that external consumers
//! (CLI trace output, web service streaming, metrics, etc.) can react to each
//! committed step without being wired into the core lifecycle systems.

use bevy_ecs::prelude::*;

use crate::plugins::eco::observe::Observation;
use crate::result::StepReasoning;
use crate::result::StepResult;

/// Event fired after a scheduling step has been committed.
///
/// Observers can subscribe to this event to receive the full reasoning for the
/// step as well as the resulting economy. The event is triggered from
/// [`apply_best_system`](crate::plugins::apply::apply_best_system) after the
/// step has been recorded in the internal logs.
#[derive(Event, Debug, Clone, PartialEq)]
pub struct SchedulerStepEvent {
    /// The step that was just committed.
    pub step: StepResult,
    /// Symbolic observation of the economy and unit state before the decision.
    pub observation: Observation,
    /// Reasoning captured for the step, including chosen action, top candidates,
    /// direction scores, and priority table.
    pub reasoning: StepReasoning,
    /// Whether the committed step already satisfies the search goal.
    pub goal_reached: bool,
}
