//! Explicit scheduler resources.
//!
//! Each piece of scheduler state is exposed as a single-purpose Bevy resource so
//! that systems declare exactly what they read or write instead of depending on a
//! monolithic [`SearchState`](crate::search::SearchState).

use bevy_ecs::prelude::Resource;

use faf_quantities::Time;
use faf_sim_shared::{BuildTask, GameEcoMetrics};

use crate::request::SearchOptions;
use crate::result::StepResult;
use crate::search::SearchTarget;

#[derive(Resource)]
pub struct GameEco {
    pub eco: GameEcoMetrics,
}

/// Global simulation clock for the scheduler.
///
/// Time advances to the next committed task completion rather than ticking
/// every frame.
#[derive(Resource)]
pub struct SchedulerClock {
    pub now: Time,
}

/// The search goal.
#[derive(Resource)]
pub struct SearchGoal(pub SearchTarget);

/// Loop control and bookkeeping for the search.
#[derive(Resource)]
pub struct SearchProgress {
    /// Number of greedy iterations completed.
    pub iteration: usize,
    /// Next available task id.
    pub next_id: u32,
    /// Whether the search has finished.
    pub done: bool,
}

/// Tasks committed to by the search so far.
#[derive(Resource, Default)]
pub struct TaskLog(pub Vec<BuildTask>);

/// Human-readable steps produced so far.
#[derive(Resource, Default)]
pub struct StepLog(pub Vec<StepResult>);

impl SearchProgress {
    /// True if the search should stop due to iteration/step limits.
    pub fn should_terminate(&self, options: &SearchOptions) -> bool {
        self.iteration >= options.max_iterations || self.step_count() >= options.max_steps
    }

    /// Number of committed steps so far.
    pub fn step_count(&self) -> usize {
        self.iteration
    }
}
