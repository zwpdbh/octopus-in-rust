//! Eco scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind, UnitRole};

use crate::algorithms::greedy;
use crate::config::SchedulerConfig;
use crate::plugins::lifecycle::SchedulerSet;
use crate::request::SearchOptions;
use crate::resources::{CurrentInventory, EconomyState, SearchGoal, SearchProgress};
use crate::result::Action;
use crate::search::{BlueprintLibraryRef, CandidateAction, CandidateScore, SearchTarget};
use crate::util::{count_mex, is_mex};

/// Plugin that registers candidate generation and evaluation for economy (mass
/// income) scheduling.
pub struct EcoSchedulingPlugin;

impl Plugin for EcoSchedulingPlugin {
    fn build(&self, app: &mut App) {
        // Register eco candidates generation/evaluation in the sets declared by
        // `SchedulerLifecyclePlugin`. The `configure_sets` call there orders
        // `GenerateCandidate -> EvaluateCandidate -> Apply` and gates the whole
        // pipeline on the `Searching` state.
        app.add_systems(
            Update,
            generate_eco_candidates_system.in_set(SchedulerSet::GenerateCandidate),
        )
        .add_systems(
            Update,
            evaluate_eco_candidates_system.in_set(SchedulerSet::EvaluateCandidate),
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

    // If the target is already reached, stop generating candidates.
    if goal.0.is_reached(&economy.current, &inventory.0) {
        return;
    }

    let library = &*library.0;
    let current_mex_count = count_mex(&inventory.0, library);
    let mex_cap = config.max_mex_count;

    // Build candidates from every owned builder.
    for (kind, count) in &inventory.0 {
        if *count == 0 {
            continue;
        }

        for target in library.buildable_by(kind) {
            if !is_eco_candidate(library, &target) {
                continue;
            }
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            commands.spawn(CandidateAction(Action::Build {
                builder: kind.clone(),
                target,
            }));
        }
    }

    // Upgrade extractors and storages. Upgrades do not increase the mex count
    // (they replace an existing unit), so the cap is not checked here. The
    // source unit provides its own build power, so we do not need to check for
    // an available engineer or ACU before proposing an upgrade.
    for (kind, count) in &inventory.0 {
        if *count == 0 {
            continue;
        }
        if let Some(target) = library.upgrade_target(kind) {
            if is_eco_candidate(library, &target) {
                commands.spawn(CandidateAction(Action::Upgrade {
                    from: kind.clone(),
                    to: target,
                }));
            }
        }
        // Cap mexes of a tier that supports it. Capping also replaces the
        // existing unit and does not increase the mex count.
        if let Some(target) = library.cap_target(kind) {
            if is_eco_candidate(library, &target) {
                commands.spawn(CandidateAction(Action::Upgrade {
                    from: kind.clone(),
                    to: target,
                }));
            }
        }
    }
}

/// Evaluate every spawned [`CandidateAction`] for eco scheduling and attach a
/// [`CandidateScore`].
///
/// The actual scoring function lives in the algorithm module so that different
/// algorithms can reuse the same ECS pipeline.
pub(crate) fn evaluate_eco_candidates_system(
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

    let SearchTarget::Eco(target) = &goal.0 else {
        return;
    };

    let library = &*library.0;

    for (entity, action) in candidates.iter() {
        let score = greedy::score_eco_candidate(
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
