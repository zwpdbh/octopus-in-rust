//! Candidate generation for unit scheduling.

use bevy_ecs::prelude::*;

use faf_blueprints::UnitKind;

use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::resources::{GameEco, SearchGoal, SearchProgress};
use crate::search::{
    spawn_build_candidates, spawn_upgrade_candidates, BlueprintLibraryRef, IdleBuilderQuery,
};
use crate::util::{count_mex_from_iter, is_mex};

/// Spawn candidate actions that move toward building the target unit.
///
/// For now this generates every legal build and upgrade action from the current
/// inventory; the evaluation step scores them by symbolic distance to the goal.
pub(crate) fn generate_unit_candidates_system(
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

    if goal.0.is_reached_from_entities(&economy.eco, &units) {
        return;
    }

    let library = &*library.0;
    let owned_kinds: Vec<UnitKind> = units.iter().map(|u| u.0.clone()).collect();
    let current_mex_count = count_mex_from_iter(&owned_kinds, library);
    let mex_cap = config.max_mex_count;

    // All legal build actions.
    for builder in &owned_kinds {
        for target in library.buildable_by(builder) {
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            spawn_build_candidates(
                &mut commands,
                library,
                builder,
                target,
                &idle_builders,
                &economy.eco,
            );
        }
    }

    // All legal upgrade and cap actions. The source unit transforms itself,
    // so no separate builder availability check is required.
    for from in &owned_kinds {
        if let Some(target) = library.upgrade_target(from) {
            spawn_upgrade_candidates(&mut commands, library, from, target, &idle_builders);
        }
        if let Some(target) = library.cap_target(from) {
            spawn_upgrade_candidates(&mut commands, library, from, target, &idle_builders);
        }
    }
}
