//! Candidate generation for eco scheduling.

use crate::algorithms::greedy;
use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::resources::{GameEco, SearchGoal, SearchProgress};
use crate::search::{BlueprintLibraryRef, IdleBuilderQuery};
use bevy_ecs::prelude::*;

/// Spawn all candidate eco actions.
///
/// The actual decision-making lives in
/// [`greedy::spawn_eco_candidates`](crate::algorithms::greedy::spawn_eco_candidates);
/// this system is only ECS glue.
pub(crate) fn generate_eco_candidates_system(
    mut commands: Commands,
    progress: Res<SearchProgress>,
    economy: Res<GameEco>,
    goal: Res<SearchGoal>,
    library: Res<BlueprintLibraryRef>,
    config: Res<SchedulerConfig>,
    units: Query<&UnitKindComp>,
    idle_builders: IdleBuilderQuery,
) {
    if progress.done {
        return;
    }

    // If the target is already reached, stop generating candidates.
    if goal.0.is_reached_from_entities(&economy.eco, &units) {
        return;
    }

    let library = &*library.0;
    greedy::spawn_eco_candidates(
        &mut commands,
        library,
        &config,
        &units,
        &idle_builders,
        &economy.eco,
    );
}
