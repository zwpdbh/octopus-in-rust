//! Core scheduler plugin and lifecycle.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_state::app::StatesPlugin;
use bevy_state::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind};
use faf_sim::runtime::EcoSnapshot;

use crate::request::{EcoTarget, SearchOptions};
use crate::result::{Schedule, ScheduleError};
use crate::search::{BlueprintLibraryRef, SearchState, SearchTarget};

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

/// Per-request init plugin that sets up a Bevy app for one scheduling run.
///
/// It registers the shared search state, blueprint library, and lifecycle
/// systems. Mode-specific plugins (such as [`EcoSchedulingPlugin`]) and
/// algorithm plugins (such as [`GreedyPlugin`]) must also be added.
pub struct SchedulerInitPlugin {
    library: Arc<BlueprintLibrary>,
    initial_eco: EcoSnapshot,
    inventory: HashMap<UnitKind, u32>,
    target: SearchTarget,
    options: SearchOptions,
}

impl SchedulerInitPlugin {
    /// Set up an eco scheduling search.
    pub fn new_eco(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: EcoTarget,
        options: SearchOptions,
    ) -> Self {
        Self {
            library,
            initial_eco,
            inventory,
            target: SearchTarget::Eco(target),
            options,
        }
    }

    /// Set up a unit scheduling search.
    pub fn new_unit(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: UnitKind,
        options: SearchOptions,
    ) -> Self {
        Self {
            library,
            initial_eco,
            inventory,
            target: SearchTarget::Unit(target),
            options,
        }
    }
}

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
            .insert_resource(SearchState::new(
                self.initial_eco,
                self.inventory.clone(),
                self.target.clone(),
                self.options.clone(),
            ))
            .insert_resource(BlueprintLibraryRef(self.library.clone()))
            .init_resource::<SchedulerResult>()
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
