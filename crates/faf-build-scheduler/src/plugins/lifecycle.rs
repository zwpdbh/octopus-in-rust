//! Core scheduler plugin and lifecycle.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::app::StatesPlugin;
use bevy_state::prelude::*;

use crate::plugins::apply::StepReasoningLog;
use crate::resources::SearchProgress;
use crate::result::{Schedule, ScheduleError, ScheduleWithReasoning};

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
    /// Observe the current world and produce symbolic conditions.
    Observe,
    /// Use observations to decide which direction the scheduler should take.
    DecideDirection,
    /// Produce candidate actions for the current state.
    GenerateCandidate,
    /// Evaluate candidate actions by simulating them.
    ///
    /// Different scheduling algorithms or evaluation modes plug in here:
    /// they assign a score to every candidate generated in the previous set.
    EvaluateCandidate,
    /// Pick the best-scored candidate and apply it to the search state.
    Apply,
}

/// Resource that carries the final result of a scheduling run once the search
/// terminates.
#[derive(Resource, Default)]
pub struct SchedulerResult {
    pub result: Option<Result<Schedule, ScheduleError>>,
}

/// Static plugin that registers the shared scheduler lifecycle and system sets.
///
/// It does **not** carry request-specific data. Callers must insert the
/// scheduler resources (`EconomyState`, `SearchGoal`, `SearchOptions`,
/// `SearchProgress`, etc.) before running the app.
pub struct SchedulerLifecyclePlugin;

impl Plugin for SchedulerLifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StatesPlugin)
            .init_state::<SchedulerState>()
            // Declare the cross-plugin scheduling pipeline.
            //
            // Mode plugins (`EcoSchedulingPlugin`, `UnitSchedulingPlugin`)
            // register their lifecycle systems in different source files. They
            // cannot `.chain()` with each other directly because they do not
            // import each other's system functions. Instead, each plugin tags
            // its systems with `.in_set(...)`, and the single `configure_sets`
            // call below orders those sets:
            //
            //     Observe -> DecideDirection -> GenerateCandidate -> EvaluateCandidate -> Apply
            //
            // This is the only place that needs to know the global pipeline.
            .configure_sets(
                Update,
                (
                    SchedulerSet::Observe,
                    SchedulerSet::DecideDirection,
                    SchedulerSet::GenerateCandidate,
                    SchedulerSet::EvaluateCandidate,
                    SchedulerSet::Apply,
                )
                    .chain()
                    // Only run the pipeline while the search is still active.
                    // Once `SearchProgress::done` becomes true, `transition_to_done`
                    // moves the state to `Done` and these systems stop running.
                    .run_if(in_state(SchedulerState::Searching)),
            )
            .add_systems(Update, transition_to_done.in_set(SchedulerSet::Apply));
    }
}

fn transition_to_done(
    progress: Res<SearchProgress>,
    mut next_state: ResMut<NextState<SchedulerState>>,
) {
    if progress.done {
        next_state.set(SchedulerState::Done);
    }
}

/// Run `app` until the search has produced a result.
///
/// The caller must ensure that the app contains a system that sets
/// `SearchProgress::done` to `true` and writes the outcome to
/// `SchedulerResult::result`. This helper is used by scheduling entry points to
/// keep the concrete Bevy wiring internal.
pub fn run_to_completion(app: &mut App) -> Result<Schedule, ScheduleError> {
    // Safety guard: cap the number of update loops so we never hang.
    let mut loops = 0;
    const MAX_LOOPS: usize = 100_000;

    while !app.world().resource::<SearchProgress>().done {
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

/// Run `app` until the search has produced a result, returning both the schedule
/// and the per-step candidate reasoning captured during the search.
pub fn run_to_completion_with_reasoning(
    app: &mut App,
) -> Result<ScheduleWithReasoning, ScheduleError> {
    let schedule = run_to_completion(app)?;
    let reasoning = app
        .world()
        .resource::<StepReasoningLog>()
        .0
        .clone();
    Ok(ScheduleWithReasoning { schedule, reasoning })
}
