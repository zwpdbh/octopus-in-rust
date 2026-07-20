//! Eco scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind, UnitRole};

use crate::plugins::scheduler::SchedulerSet;
use crate::result::Action;
use crate::search::{
    score_result, simulate_with_action, BlueprintLibraryRef, CandidateAction, CandidateScore,
    SearchState, SearchTarget,
};

/// Plugin that registers candidate generation and evaluation for economy (mass
/// income) scheduling.
pub struct EcoSchedulingPlugin;

impl Plugin for EcoSchedulingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            generate_eco_candidates_system.in_set(SchedulerSet::Generate),
        )
        .add_systems(
            Update,
            evaluate_eco_candidates_system.in_set(SchedulerSet::Evaluate),
        );
    }
}

/// Spawn candidate actions for increasing mass income.
///
/// Candidates include building any unit that contributes to mass production or
/// storage, as well as upgrading extractors/storages when higher tiers are
/// available.
pub(crate) fn generate_eco_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
) {
    if state.done {
        return;
    }

    // If the target is already reached, stop generating candidates.
    if state
        .target
        .is_reached(&state.current_eco, &state.inventory)
    {
        return;
    }

    let library = &*library.0;

    // Build candidates from every owned builder.
    for (kind, count) in &state.inventory {
        if *count == 0 {
            continue;
        }

        for target in library.buildable_by(kind) {
            if is_eco_candidate(library, &target) {
                commands.spawn(CandidateAction(Action::Build {
                    builder: kind.clone(),
                    target,
                }));
            }
        }
    }

    // Upgrade extractors and storages.
    for (kind, count) in &state.inventory {
        if *count == 0 {
            continue;
        }
        for path in library.upgrade_paths(kind) {
            if is_eco_candidate(library, &path.target) {
                for builder in &path.builders {
                    if state.inventory.get(builder).copied().unwrap_or(0) > 0 {
                        commands.spawn(CandidateAction(Action::Upgrade {
                            from: kind.clone(),
                            to: path.target.clone(),
                            builder: builder.clone(),
                        }));
                    }
                }
            }
        }
    }
}

/// Evaluate every spawned [`CandidateAction`] for eco scheduling and attach a
/// [`CandidateScore`].
pub(crate) fn evaluate_eco_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction)>,
) {
    if state.done {
        return;
    }

    let SearchTarget::Eco(target) = &state.target else {
        return;
    };

    let library = &*library.0;

    for (entity, action) in candidates.iter() {
        let score = if let Some(result) = simulate_with_action(&state, &action.0, library) {
            let completion = result.tasks.last().cloned().unwrap_or(result.total);
            score_result(
                &completion,
                target,
                state.options.simulation_max_time_seconds,
            )
        } else {
            f64::INFINITY
        };

        commands.entity(entity).insert(CandidateScore(score));
    }
}

fn is_eco_candidate(library: &BlueprintLibrary, kind: &UnitKind) -> bool {
    matches!(
        library.role(kind),
        UnitRole::MassExtractor
            | UnitRole::PowerGenerator
            | UnitRole::EnergyStorage
            | UnitRole::Engineer
            | UnitRole::Factory
    )
}
