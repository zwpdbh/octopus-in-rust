//! Explicit scheduler resources.
//!
//! Each piece of scheduler state is exposed as a single-purpose Bevy resource so
//! that systems declare exactly what they read or write instead of depending on a
//! monolithic [`SearchState`](crate::search::SearchState).

use std::collections::HashMap;

use bevy_ecs::prelude::Resource;

use faf_blueprints::{TechLevel, UnitKind};
use faf_sim_shared::{BuildTask, EcoSnapshot};

use crate::request::SearchOptions;
use crate::result::StepResult;
use crate::search::SearchTarget;

/// Economy snapshots for the current scheduling run.
///
/// `initial` is the snapshot the search started from and is used to build the
/// final construction plan. `current` is the snapshot after the steps committed
/// so far and is what candidate generation and simulation operate on.
#[derive(Resource)]
pub struct EconomyState {
    pub initial: EcoSnapshot,
    pub current: EcoSnapshot,
}

/// Units currently owned by the player.
#[derive(Resource)]
pub struct CurrentInventory(pub HashMap<UnitKind, u32>);

/// Highest technology tier currently available, derived from owned engineers.
///
/// If no engineer is owned the tier defaults to [`TechLevel::T1`].
#[derive(Resource)]
pub struct CurrentTechLevel(pub TechLevel);

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
