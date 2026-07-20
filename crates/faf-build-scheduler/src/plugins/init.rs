//! Core scheduler plugin and lifecycle.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::app::StatesPlugin;
use bevy_state::prelude::*;

use crate::result::{Schedule, ScheduleError};
use crate::search::SearchState;

/// Lifecycle states of a scheduling search.
///
/// The search starts in [`Searching`](SchedulerState::Searching) and transitions
/// to [`Done`](SchedulerState::Done) once a result has been produced.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SchedulerState {
    #[default]
    Searching,
    Done,
}

/// System sets used to order the phases of a scheduling search.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub enum SchedulerSet {
    /// Produce candidate actions for the current state.
    Generate,
    /// Evaluate candidate actions by simulating them.
    Evaluate,
    /// Commit the best candidate and check for termination.
    Select,
}

/// Resource that carries the final result of a scheduling run once the search
/// terminates.
#[derive(Resource, Default)]
pub struct SchedulerResult {
    pub result: Option<Result<Schedule, ScheduleError>>,
}

/// Static plugin that registers the shared scheduler lifecycle and system sets.
///
/// It does **not** carry request-specific data. Callers must insert
/// [`SearchState`] and [`BlueprintLibraryRef`] resources before running the app.
pub struct SchedulerInitPlugin;

impl Plugin for SchedulerInitPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StatesPlugin)
            .init_state::<SchedulerState>()
            .configure_sets(
                Update,
                (
                    SchedulerSet::Generate,
                    SchedulerSet::Evaluate,
                    SchedulerSet::Select,
                )
                    .chain()
                    .run_if(in_state(SchedulerState::Searching)),
            )
            .add_systems(Update, transition_to_done.in_set(SchedulerSet::Select));
    }
}

fn transition_to_done(state: Res<SearchState>, mut next_state: ResMut<NextState<SchedulerState>>) {
    if state.done {
        next_state.set(SchedulerState::Done);
    }
}

/// Run `app` until the search has produced a result.
///
/// The caller must ensure that the app contains a system that sets
/// `SearchState::done` to `true` and writes the outcome to
/// `SchedulerResult::result`. This helper is used by scheduling entry points to
/// keep the concrete Bevy wiring internal.
pub fn run_to_completion(app: &mut App) -> Result<Schedule, ScheduleError> {
    // Safety guard: cap the number of update loops so we never hang.
    let mut loops = 0;
    const MAX_LOOPS: usize = 100_000;

    while !app.world().resource::<SearchState>().done {
        app.update();
        loops += 1;
        if loops >= MAX_LOOPS {
            return Err(ScheduleError::SearchTimeout);
        }
    }

    app.world()
        .resource::<SchedulerResult>()
        .result
        .clone()
        .unwrap_or(Err(ScheduleError::GoalUnreachable))
}
