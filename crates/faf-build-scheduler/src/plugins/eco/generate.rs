//! Candidate generation for eco scheduling.

use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, TechLevel, UnitKind, UnitRole};

use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::resources::{CurrentInventory, EconomyState, SearchGoal, SearchProgress};
use crate::search::{
    spawn_build_candidates, spawn_upgrade_candidates, BlueprintLibraryRef, IdleBuilderQuery,
};
use crate::util::{count_mex_from_iter, is_mex};

/// Spawn candidate actions for increasing mass income.
///
/// Candidates include building any unit that contributes to mass production or
/// storage, as well as upgrading extractors/storages when higher tiers are
/// available.
pub(crate) fn generate_eco_candidates_system(
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

    // If the target is already reached, stop generating candidates.
    if goal.0.is_reached_from_entities(&economy.current, &units) {
        return;
    }

    let library = &*library.0;
    let owned_kinds: Vec<UnitKind> = units.iter().map(|u| u.0.clone()).collect();
    let current_mex_count = count_mex_from_iter(&owned_kinds, library);
    let mex_cap = config.max_mex_count;

    // Opening-phase constraints: a human-like FAF opening requires a factory
    // before the ACU builds economy, and a few engineers before the economy
    // expansion really starts.
    let has_factory = owned_kinds
        .iter()
        .any(|k| matches!(k, UnitKind::Factory(_)));
    let engineer_count = owned_kinds
        .iter()
        .filter(|k| matches!(k, UnitKind::Engineer(_)))
        .count() as u32;
    const MIN_OPENING_ENGINEERS: u32 = 2;

    // Phase 0: no factory yet => ACU must build a T1 factory first.
    if !has_factory {
        if owned_kinds.iter().any(|k| *k == UnitKind::Commander) {
            if let Some(target) = library
                .buildable_by(&UnitKind::Commander)
                .into_iter()
                .find(|t| matches!(t, UnitKind::Factory(TechLevel::T1)))
            {
                spawn_build_candidates(
                    &mut commands,
                    library,
                    &UnitKind::Commander,
                    target,
                    &idle_builders,
                );
            }
        }
        // No other build candidates until the factory exists.
        return;
    }

    // Phase 1: factory exists but we still need opening engineers => factories
    // must produce engineers. The ACU waits; this models the factory working on
    // engineers while the ACU is free to scout/protect but not expand economy
    // yet.
    if engineer_count < MIN_OPENING_ENGINEERS {
        for kind in owned_kinds
            .iter()
            .filter(|k| matches!(k, UnitKind::Factory(_)))
        {
            if let Some(target) = library
                .buildable_by(kind)
                .into_iter()
                .find(|t| matches!(t, UnitKind::Engineer(_)))
            {
                spawn_build_candidates(&mut commands, library, kind, target, &idle_builders);
            }
        }
        return;
    }

    // Phase 2: normal economy expansion.
    for kind in &owned_kinds {
        for target in library.buildable_by(kind) {
            if !is_eco_candidate(library, &target) {
                continue;
            }
            // Once engineers are available, the ACU focuses on power/factories
            // while engineers handle mex expansion.
            if *kind == UnitKind::Commander && is_mex(library, &target) {
                continue;
            }
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            spawn_build_candidates(&mut commands, library, kind, target, &idle_builders);
        }
    }

    // Upgrade extractors and storages. Upgrades do not increase the mex count
    // (they replace an existing unit), so the cap is not checked here. The
    // source unit provides its own build power, so we do not need to check for
    // an available engineer or ACU before proposing an upgrade.
    for kind in &owned_kinds {
        if let Some(target) = library.upgrade_target(kind) {
            if is_eco_candidate(library, &target) {
                spawn_upgrade_candidates(&mut commands, library, kind, target, &idle_builders);
            }
        }
        // Cap mexes of a tier that supports it. Capping also replaces the
        // existing unit and does not increase the mex count.
        if let Some(target) = library.cap_target(kind) {
            if is_eco_candidate(library, &target) {
                spawn_upgrade_candidates(&mut commands, library, kind, target, &idle_builders);
            }
        }
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
