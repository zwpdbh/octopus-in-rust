//! Greedy best-first scheduling algorithm.

use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use faf_blueprints::UnitEcoStats;
use faf_blueprints::{BlueprintGraph, BlueprintLibrary, TechLevel, UnitKind};
use faf_sim_shared::EcoSnapshot;

use crate::algorithms::heuristic;
use crate::algorithms::SchedulingAlgorithm;
use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::plugins::eco::decide_direction::{DirectionScores, PriorityTable};
use crate::plugins::eco::observe::{
    compute_energy_margin, compute_mass_margin, EnergyMargin, MassMargin,
};
use crate::request::SearchOptions;
use crate::result::Action;
use crate::search::{
    solve_action, spawn_build_candidates, spawn_upgrade_candidates, IdleBuilderQuery,
};
use crate::util::{count_mex_from_iter, is_mex};

/// Assignment of concrete builders to a candidate action.
pub(crate) type CandidateBuilders = Vec<(
    bevy_ecs::prelude::Entity,
    faf_blueprints::UnitKind,
    UnitEcoStats,
)>;

/// Minimum energy storage ratio allowed after committing a candidate. If a build
/// would drop the buffer below this, the candidate is rejected.
const POST_ACTION_ENERGY_STORAGE_THRESHOLD: f64 = 0.30;
/// Minimum mass storage ratio allowed after committing a candidate.
const POST_ACTION_MASS_STORAGE_THRESHOLD: f64 = 0.20;

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
                spawn_build_candidates(commands, library, kind, target, idle_builders);
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
            spawn_build_candidates(commands, library, kind, target, idle_builders);
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
    current_economy: &EcoSnapshot,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &CandidateBuilders,
    library: &BlueprintLibrary,
    scores: &DirectionScores,
    priorities: &PriorityTable,
) -> f64 {
    let Some(result) = solve_action(
        current_economy,
        next_id,
        options,
        action,
        assigned_builders,
        library,
    ) else {
        return 0.0;
    };
    let completion = result.tasks.last().cloned().unwrap_or(result.total);

    // Lookahead guard: reject actions that would leave the economy in a bad
    // state. This is where we "try the next action" until a healthy one is
    // found — bad candidates simply score zero and are ignored by apply.
    let energy_after = compute_energy_margin(&completion.economy);
    let mass_after = compute_mass_margin(&completion.economy);
    if matches!(
        energy_after,
        EnergyMargin::Stalled | EnergyMargin::Thin | EnergyMargin::Unhealthy
    ) || matches!(mass_after, MassMargin::Stall)
    {
        return 0.0;
    }

    // Storage-buffer guard: reject actions that would leave storage too low,
    // because the schedule cannot predict intermediate drain and a low buffer
    // risks an in-flight stall.
    let post_energy_ratio = storage_ratio(
        completion.economy.energy_storage.value(),
        completion.economy.energy_storage_cap.value(),
    );
    let post_mass_ratio = storage_ratio(
        completion.economy.mass_storage.value(),
        completion.economy.mass_storage_cap.value(),
    );
    if post_energy_ratio < POST_ACTION_ENERGY_STORAGE_THRESHOLD
        || post_mass_ratio < POST_ACTION_MASS_STORAGE_THRESHOLD
    {
        return 0.0;
    }

    // Categorize the action and look up the corresponding confidence score and
    // resource priority.
    let (confidence, efficiency, priority) = if let Some(mass) =
        heuristic::mass_income_efficiency(current_economy, &completion, action, library)
    {
        (scores.mass_income, mass, priorities.mass)
    } else if let Some(energy) =
        heuristic::energy_income_efficiency(current_economy, &completion, action, library)
    {
        (scores.energy, energy, priorities.energy)
    } else if let Some(tier) = heuristic::engineer_tier(action) {
        (
            scores.build_power,
            (tier as i32 + 1) as f64,
            priorities.build_power,
        )
    } else if heuristic::is_tech_upgrade_to(action, TechLevel::T3) {
        (scores.tech_t3, 0.0, 5)
    } else if heuristic::is_tech_upgrade_to(action, TechLevel::T2) {
        (scores.tech_t2, 0.0, 5)
    } else {
        (0, 0.0, 5)
    };

    if confidence == 0 {
        return 0.0;
    }

    // Confidence drives which direction wins and the priority table scales the
    // whole score up or down based on current resource health.
    let base = confidence as f64 * 100.0 + efficiency - completion.time_seconds * 1e-9;
    base * (priority as f64 / 5.0)
}

/// Score a unit candidate by symbolic distance to the target unit.
///
/// Candidates that directly build the target use the simulated completion time;
/// all others are ranked by how many build/upgrade edges separate their result
/// from the goal. Higher scores are better.
pub(crate) fn score_unit_candidate(
    current_economy: &EcoSnapshot,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &CandidateBuilders,
    library: &BlueprintLibrary,
    target: &UnitKind,
) -> f64 {
    let graph = library.build_graph();
    let max_time = options.simulation_max_time_seconds;
    let resulting_unit = heuristic::resulting_unit(action);

    if resulting_unit == *target {
        if let Some(result) = solve_action(
            current_economy,
            next_id,
            options,
            action,
            assigned_builders,
            library,
        ) {
            let completion = result.tasks.last().cloned().unwrap_or(result.total);
            -completion.time_seconds
        } else {
            f64::NEG_INFINITY
        }
    } else {
        match distance_to_target(&graph, &resulting_unit, target) {
            Some(distance) => -(max_time + distance as f64),
            None => f64::NEG_INFINITY,
        }
    }
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
