//! Unit scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use faf_blueprints::UnitKind;

use crate::algorithms::greedy::score_unit_candidate;
use crate::components::{CandidateAssignment, UnitKindComp};
use crate::config::SchedulerConfig;
use crate::plugins::lifecycle::SchedulerSet;
use crate::request::SearchOptions;
use crate::resources::{CurrentInventory, EconomyState, SearchGoal, SearchProgress};
use crate::search::{
    spawn_build_candidates, spawn_upgrade_candidates, BlueprintLibraryRef, CandidateAction,
    CandidateScore, IdleBuilderQuery, SearchTarget,
};
use crate::util::{count_mex_from_iter, is_mex};

/// Plugin that registers candidate generation and evaluation for unit
/// scheduling.
pub struct UnitSchedulingPlugin;

impl Plugin for UnitSchedulingPlugin {
    fn build(&self, app: &mut App) {
        // Register unit candidate generation/evaluation in the sets declared by
        // `SchedulerLifecyclePlugin`. The global pipeline order and state gate are
        // configured there, not here.
        app.add_systems(
            Update,
            generate_unit_candidates_system.in_set(SchedulerSet::GenerateCandidate),
        )
        .add_systems(
            Update,
            evaluate_unit_candidates_system.in_set(SchedulerSet::EvaluateCandidate),
        );
    }
}

/// Spawn candidate actions that move toward building the target unit.
///
/// For now this generates every legal build and upgrade action from the current
/// inventory; the evaluation step scores them by symbolic distance to the goal.
pub(crate) fn generate_unit_candidates_system(
    mut commands: Commands,
    progress: Res<SearchProgress>,
    economy: Res<EconomyState>,
    _inventory: Res<CurrentInventory>,
    goal: Res<SearchGoal>,
    library: Res<BlueprintLibraryRef>,
    config: Res<SchedulerConfig>,
    units: Query<&UnitKindComp>,
    idle_builders: IdleBuilderQuery,
) {
    if progress.done {
        return;
    }

    if goal.0.is_reached_from_entities(&economy.current, &units) {
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
            spawn_build_candidates(&mut commands, library, builder, target, &idle_builders);
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

/// Evaluate every spawned [`CandidateAction`] for unit scheduling and attach a
/// [`CandidateScore`].
///
/// The actual scoring function lives in the algorithm module so that different
/// algorithms can reuse the same ECS pipeline.
pub(crate) fn evaluate_unit_candidates_system(
    mut commands: Commands,
    progress: Res<SearchProgress>,
    economy: Res<EconomyState>,
    goal: Res<SearchGoal>,
    options: Res<SearchOptions>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction, &CandidateAssignment)>,
) {
    if progress.done {
        return;
    }

    let SearchTarget::Unit(target) = &goal.0 else {
        return;
    };

    let library = &*library.0;

    for (entity, action, assignment) in candidates.iter() {
        let score = score_unit_candidate(
            &economy.current,
            progress.next_id,
            &options,
            &action.0,
            &assignment.0,
            library,
            target,
        );
        commands.entity(entity).insert(CandidateScore(score));
    }
}
