//! Bevy ECS resources used by the economy runtime.

use bevy_ecs::prelude::*;

use crate::economy::GameEcoMetrics;
use crate::quantities::Time;
use crate::runtime::types::{BuildTask, ScheduledTask, SimulationEvent};

/// Current simulation time and step size.
#[derive(Resource)]
pub(crate) struct SimClock {
    pub(crate) time: Time,
    pub(crate) dt: Time,
    pub(crate) max_time: Option<Time>,
}

/// Tasks waiting to become active. The first task becomes ready when its
/// `ready_at` time is reached.
#[derive(Resource)]
pub(crate) struct PendingTasks(pub(crate) Vec<ScheduledTask>);

impl PendingTasks {
    /// Schedule a queue of tasks so that each task starts `start_after` seconds
    /// after the previous task finishes. The first task is delayed relative to
    /// time 0.
    pub(crate) fn from_tasks(tasks: Vec<BuildTask>) -> Self {
        let scheduled = tasks
            .into_iter()
            .enumerate()
            .map(|(index, task)| {
                let ready_at = if index == 0 {
                    task.start_after
                } else {
                    Time::from_raw(f64::INFINITY)
                };
                ScheduledTask::new(task, ready_at)
            })
            .collect();
        Self(scheduled)
    }
}

/// Entities whose current construction target finished this tick.
#[derive(Resource)]
pub(crate) struct CompletedTasks(pub(crate) Vec<Entity>);

/// Global stall factor computed by the economy system and consumed by the
/// progress system.
#[derive(Resource)]
pub(crate) struct EffectiveFactor(pub(crate) f64);

/// Event log collected during the current update.
#[derive(Resource, Default)]
pub(crate) struct EventJournal(pub(crate) Vec<SimulationEvent>);

/// True once the queue is empty or `max_time` is reached.
#[derive(Resource, Default)]
pub(crate) struct FinishedFlag(pub(crate) bool);

/// Current economy state, mirrored from [`EconomyRuntimeState`].
#[derive(Resource)]
pub(crate) struct EcoState(pub(crate) GameEcoMetrics);

/// Cumulative resources spent on construction.
#[derive(Resource)]
pub(crate) struct TotalsSpent {
    pub(crate) mass: f64,
    pub(crate) energy: f64,
}

/// When the queue becomes empty, the simulation keeps ticking until this time.
#[derive(Resource, Default)]
pub(crate) struct TailEndTime(pub(crate) Option<Time>);

/// Optional post-queue tail duration.
///
/// `None` means the simulation finishes immediately when the queue is empty.
/// `Some(seconds)` keeps the clock running for that many seconds after the
/// queue empties so the final economy state remains visible in charts.
#[derive(Resource)]
pub(crate) struct PostQueueTailSeconds(pub(crate) Option<f64>);
