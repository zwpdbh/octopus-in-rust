//! Unit scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::algorithms::greedy::score_unit_candidate;
use crate::config::SchedulerConfig;
use crate::plugins::lifecycle::SchedulerSet;
use crate::result::Action;
use crate::search::{
    BlueprintLibraryRef, CandidateAction, CandidateScore, SearchState, SearchTarget,
};
use crate::util::{count_mex, is_mex};

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
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    config: Res<SchedulerConfig>,
) {
    if state.done {
        return;
    }

    if state
        .target
        .is_reached(&state.current_eco, &state.inventory)
    {
        return;
    }

    let library = &*library.0;
    let current_mex_count = count_mex(&state.inventory, library);
    let mex_cap = config.max_mex_count;

    // All legal build actions.
    for (builder, count) in &state.inventory {
        if *count == 0 {
            continue;
        }
        for target in library.buildable_by(builder) {
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            commands.spawn(CandidateAction(Action::Build {
                builder: builder.clone(),
                target,
            }));
        }
    }

    // All legal upgrade actions.
    for (from, count) in &state.inventory {
        if *count == 0 {
            continue;
        }
        for path in library.upgrade_paths(from) {
            for builder in &path.builders {
                if state.inventory.get(builder).copied().unwrap_or(0) > 0 {
                    commands.spawn(CandidateAction(Action::Upgrade {
                        from: from.clone(),
                        to: path.target.clone(),
                        builder: builder.clone(),
                    }));
                }
            }
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
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction)>,
) {
    if state.done {
        return;
    }

    let SearchTarget::Unit(target) = &state.target else {
        return;
    };

    let library = &*library.0;

    for (entity, action) in candidates.iter() {
        let score = score_unit_candidate(&state, &action.0, library, target);
        commands.entity(entity).insert(CandidateScore(score));
    }
}
