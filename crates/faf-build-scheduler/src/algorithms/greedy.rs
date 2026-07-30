//! Greedy best-first scheduling algorithm.
#![allow(unused)]
use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use faf_blueprints::UnitEcoStats;
use faf_blueprints::{BlueprintGraph, BlueprintLibrary, TechLevel, UnitKind};
use faf_sim_shared::{GameEcoMetrics, PlayerEcoSnapshot};

use crate::algorithms::heuristic;
use crate::algorithms::SchedulingAlgorithm;
use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::plugins::eco::decide_direction::{DirectionScores, PriorityTable};
use crate::plugins::eco::observe::MassMargin;
use crate::request::SearchOptions;
use crate::result::Action;
use crate::search::{
    solve_action, spawn_build_candidates, spawn_upgrade_candidates, CandidateScore,
    IdleBuilderQuery,
};
use crate::util::{count_mex_from_iter, is_mex};
use faf_sim_shared::{CandidateScoreBreakdown, ScoreCategory};

/// Assignment of concrete builders to a candidate action.
pub(crate) type CandidateBuilders = Vec<(
    bevy_ecs::prelude::Entity,
    faf_blueprints::UnitKind,
    UnitEcoStats,
)>;

/// Minimum energy storage ratio allowed after committing a candidate. A small
/// buffer is enough because the solver already verified the build can finish
/// without stalling; requiring a large buffer rejected every mass/energy build.
const POST_ACTION_ENERGY_STORAGE_THRESHOLD: f64 = 0.05;
/// Minimum mass storage ratio allowed after committing a candidate. Mass is
/// meant to be spent, so only a literal empty buffer is forbidden here.
const POST_ACTION_MASS_STORAGE_THRESHOLD: f64 = 0.0;

fn storage_ratio(current: f64, cap: f64) -> f64 {
    if cap > 0.0 {
        current / cap
    } else {
        0.0
    }
}

/// Greedy search: at each iteration, generate candidates, simulate them, and
/// commit the lowest-scoring candidate.
#[derive(Debug, Default, Clone, Copy)]
pub struct Greedy;

impl SchedulingAlgorithm for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn configure_app(&self, _app: &mut App) {
        // The apply step is registered by the scheduling-mode plugin, so the
        // greedy algorithm itself only needs to provide the scoring helpers.
    }
}

/// Spawn eco candidates for the greedy search.
///
/// The actual decision logic lives here so that the ECS system in
/// `plugins::eco::generate` is only thin glue. All legal build/upgrade actions
/// are spawned; the evaluate step uses direction confidence scores to rank them.
pub(crate) fn spawn_eco_candidates(
    commands: &mut Commands,
    library: &BlueprintLibrary,
    config: &SchedulerConfig,
    units: &Query<&UnitKindComp>,
    idle_builders: &IdleBuilderQuery,
    eco_snapshot: &GameEcoMetrics,
) {
    let owned_kinds: Vec<UnitKind> = units.iter().map(|u| u.0.clone()).collect();
    let current_mex_count = count_mex_from_iter(&owned_kinds, library);
    let unique_kinds: std::collections::HashSet<&UnitKind> = owned_kinds.iter().collect();
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
        if owned_kinds.contains(&UnitKind::Commander) {
            if let Some(target) = library
                .buildable_by(&UnitKind::Commander)
                .into_iter()
                .find(|t| matches!(t, UnitKind::Factory(TechLevel::T1)))
            {
                spawn_build_candidates(
                    commands,
                    library,
                    &UnitKind::Commander,
                    target,
                    idle_builders,
                    eco_snapshot,
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
                spawn_build_candidates(
                    commands,
                    library,
                    kind,
                    target,
                    idle_builders,
                    eco_snapshot,
                );
            }
        }
        return;
    }

    // Phase 2: generate all legal build and upgrade/cap candidates. The evaluate
    // step will rank them using per-direction confidence scores and a lookahead
    // guard.
    for kind in unique_kinds {
        for target in library.buildable_by(kind) {
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            spawn_build_candidates(commands, library, kind, target, idle_builders, eco_snapshot);
        }
        if let Some(target) = library.upgrade_target(kind) {
            spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
        }
        if let Some(target) = library.cap_target(kind) {
            spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
        }
    }
}

/// Score an eco candidate using per-direction confidence and a one-step
/// lookahead guard.
///
/// Actions whose simulated result would leave the economy stalled or critically
/// thin are scored as `0.0` so the apply step naturally skips them. Otherwise,
/// the score combines the direction confidence (0–100) with the action’s
/// efficiency.
pub(crate) fn score_eco_candidate(
    current_economy: &GameEcoMetrics,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &CandidateBuilders,
    library: &BlueprintLibrary,
    scores: &DirectionScores,
    priorities: &PriorityTable,
) -> CandidateScore {
    todo!("not implemented")
}

/// Score a unit candidate by symbolic distance to the target unit.
///
/// Candidates that directly build the target use the simulated completion time;
/// all others are ranked by how many build/upgrade edges separate their result
/// from the goal. Higher scores are better.
pub(crate) fn score_unit_candidate(
    current_economy: &GameEcoMetrics,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &CandidateBuilders,
    library: &BlueprintLibrary,
    target: &UnitKind,
) -> CandidateScore {
    todo!("not implemented")
}

/// Shortest number of build/upgrade steps from `from` to `target` in the
/// symbolic blueprint graph. Returns `None` if the target is unreachable.
fn distance_to_target(graph: &BlueprintGraph, from: &UnitKind, target: &UnitKind) -> Option<usize> {
    if from == target {
        return Some(0);
    }

    let start = graph.node_index(from)?;
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back((start, 0usize));

    while let Some((idx, dist)) = queue.pop_front() {
        let kind = &graph.graph[idx].kind;
        if kind == target {
            return Some(dist);
        }

        for neighbor in graph
            .builds_by(kind)
            .map(|(n, _)| n)
            .chain(graph.upgrades_from(kind).map(|(n, _)| n))
        {
            if visited.insert(neighbor) {
                queue.push_back((neighbor, dist + 1));
            }
        }
    }

    None
}
