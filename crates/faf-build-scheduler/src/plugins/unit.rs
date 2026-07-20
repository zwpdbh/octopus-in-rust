//! Unit scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::algorithms::greedy::score_unit_candidate;
use crate::config::SchedulerConfig;
use crate::plugins::lifecycle::SchedulerSet;
use crate::request::SearchOptions;
use crate::resources::{CurrentInventory, EconomyState, SearchGoal, SearchProgress};
use crate::result::Action;
use crate::search::{BlueprintLibraryRef, CandidateAction, CandidateScore, SearchTarget};
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
    progress: Res<SearchProgress>,
    economy: Res<EconomyState>,
    inventory: Res<CurrentInventory>,
    goal: Res<SearchGoal>,
    library: Res<BlueprintLibraryRef>,
    config: Res<SchedulerConfig>,
) {
    if progress.done {
        return;
    }

    if goal.0.is_reached(&economy.current, &inventory.0) {
        return;
    }

    let library = &*library.0;
    let current_mex_count = count_mex(&inventory.0, library);
    let mex_cap = config.max_mex_count;

    // All legal build actions.
    for (builder, count) in &inventory.0 {
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
    for (from, count) in &inventory.0 {
        if *count == 0 {
            continue;
        }
        for path in library.upgrade_paths(from) {
            for builder in &path.builders {
                if inventory.0.get(builder).copied().unwrap_or(0) > 0 {
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
    progress: Res<SearchProgress>,
    economy: Res<EconomyState>,
    inventory: Res<CurrentInventory>,
    goal: Res<SearchGoal>,
    options: Res<SearchOptions>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction)>,
) {
    if progress.done {
        return;
    }

    let SearchTarget::Unit(target) = &goal.0 else {
        return;
    };

    let library = &*library.0;

    for (entity, action) in candidates.iter() {
        let score = score_unit_candidate(
            &economy.current,
            &inventory.0,
            progress.next_id,
            &options,
            &action.0,
            library,
            target,
        );
        commands.entity(entity).insert(CandidateScore(score));
    }
}
