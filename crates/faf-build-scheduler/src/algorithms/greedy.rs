//! Greedy best-first scheduling algorithm.

use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use faf_blueprints::UnitEcoStats;
use faf_blueprints::{BlueprintGraph, BlueprintLibrary, TechLevel, UnitKind, UnitRole};
use faf_sim_shared::EcoSnapshot;

use crate::algorithms::heuristic;
use crate::algorithms::SchedulingAlgorithm;
use crate::components::UnitKindComp;
use crate::config::SchedulerConfig;
use crate::decision::EcoDirection;
use crate::plugins::apply::ApplyPlugin;
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

/// Greedy search: at each iteration, generate candidates, simulate them, and
/// commit the lowest-scoring candidate.
#[derive(Debug, Default, Clone, Copy)]
pub struct Greedy;

impl SchedulingAlgorithm for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn configure_app(&self, app: &mut App) {
        app.add_plugins(ApplyPlugin);
    }
}

/// Minimum net energy income (production - demand) to maintain before switching
/// back to mass/tech expansion. Building pgens preemptively prevents the economy
/// from stalling when mex maintenance rises.
const ENERGY_BUFFER: f64 = 10.0;
/// Large positive bonus applied to tech upgrades when tech is the active
/// direction, so they are chosen over any fallback action.
const TECH_PRIORITY_BONUS: f64 = 1_000_000.0;
/// Bonus applied to mass-income actions when mass is the active direction,
/// ensuring they outrank energy fallback candidates.
const MASS_DIRECTION_BONUS: f64 = 1_000.0;
/// Bonus applied to energy actions when energy is the active direction.
const ENERGY_DIRECTION_BONUS: f64 = 1_000.0;
/// Bonus applied to engineer actions when build power is the active direction.
const BUILD_POWER_BONUS: f64 = 1_000.0;

/// Spawn eco candidates according to the greedy opening and expansion rules.
///
/// The actual decision logic lives here so that the ECS system in
/// `plugins::eco::generate` is only thin glue.
pub(crate) fn spawn_eco_candidates(
    commands: &mut Commands,
    library: &BlueprintLibrary,
    config: &SchedulerConfig,
    direction: EcoDirection,
    units: &Query<&UnitKindComp>,
    idle_builders: &IdleBuilderQuery,
) {
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

    // Phase 2: generate candidates that correspond to the decided direction.
    match direction {
        EcoDirection::MassIncome => {
            for kind in &owned_kinds {
                for target in library.buildable_by(kind) {
                    if !is_mex(library, &target) {
                        continue;
                    }
                    // Once engineers are available, the ACU focuses on
                    // power/factories while engineers handle mex expansion.
                    if *kind == UnitKind::Commander {
                        continue;
                    }
                    // Enforce the global mex cap on *new* mass extractors.
                    if current_mex_count >= mex_cap {
                        continue;
                    }
                    spawn_build_candidates(commands, library, kind, target, idle_builders);
                }
                if let Some(target) = library.upgrade_target(kind) {
                    if is_mex(library, &target) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
                if let Some(target) = library.cap_target(kind) {
                    if is_mex(library, &target) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
            }
        }
        EcoDirection::Energy => {
            for kind in &owned_kinds {
                for target in library.buildable_by(kind) {
                    let role = library.role(&target);
                    if matches!(role, UnitRole::PowerGenerator | UnitRole::EnergyStorage) {
                        spawn_build_candidates(commands, library, kind, target, idle_builders);
                    }
                }
            }
        }
        EcoDirection::Tech(desired_tech) => {
            for kind in &owned_kinds {
                if let Some(target) = library.upgrade_target(kind) {
                    if heuristic::is_tech_upgrade_to(
                        &Action::Upgrade {
                            from: kind.clone(),
                            to: target.clone(),
                            assisted_by: vec![],
                        },
                        desired_tech,
                    ) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
                if let Some(target) = library.cap_target(kind) {
                    if heuristic::is_tech_upgrade_to(
                        &Action::Upgrade {
                            from: kind.clone(),
                            to: target.clone(),
                            assisted_by: vec![],
                        },
                        desired_tech,
                    ) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
            }
        }
        EcoDirection::BuildPower => {
            for kind in &owned_kinds {
                for target in library.buildable_by(kind) {
                    if matches!(target, UnitKind::Engineer(_)) {
                        spawn_build_candidates(commands, library, kind, target, idle_builders);
                    }
                }
                if let Some(target) = library.upgrade_target(kind) {
                    if matches!(target, UnitKind::Engineer(_)) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
                if let Some(target) = library.cap_target(kind) {
                    if matches!(target, UnitKind::Engineer(_)) {
                        spawn_upgrade_candidates(commands, library, kind, target, idle_builders);
                    }
                }
            }
        }
    }
}

/// Score an eco candidate according to the current scheduling direction.
///
/// Only actions that match the direction receive a positive score (higher is
/// better). Unrelated or infeasible actions are scored as `0.0` so they are not
/// chosen while a matching candidate exists.
pub(crate) fn score_eco_candidate(
    current_economy: &EcoSnapshot,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &CandidateBuilders,
    library: &BlueprintLibrary,
    direction: EcoDirection,
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

    // Hard guard: never choose an action that would leave the economy with too
    // thin an energy margin, unless the action itself increases energy
    // production. This prevents the scheduler from spending itself into an
    // energy stall it cannot build out of.
    let energy_demand_after = completion.economy.maintenance_consumption_per_second_energy
        + completion.economy.energy_drain;
    let net_energy_after = completion.economy.production_per_second_energy - energy_demand_after;
    let delta_energy = completion.economy.production_per_second_energy
        - current_economy.production_per_second_energy;
    if net_energy_after < ENERGY_BUFFER && delta_energy <= 0.0 {
        return 0.0;
    }

    match direction {
        EcoDirection::Tech(desired_tech) => {
            if heuristic::is_tech_upgrade_to(action, desired_tech) {
                // Tech upgrades beat every fallback. Faster upgrades are slightly
                // preferred among themselves.
                return TECH_PRIORITY_BONUS - completion.time_seconds * 1e-9;
            }
            // If the desired tech upgrade is stalled by energy, fall back to
            // building energy so we can tech later.
            if let Some(energy) =
                heuristic::energy_income_efficiency(current_economy, &completion, action, library)
            {
                return energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
        EcoDirection::MassIncome => {
            if let Some(mass) =
                heuristic::mass_income_efficiency(current_economy, &completion, action, library)
            {
                return MASS_DIRECTION_BONUS + mass - completion.time_seconds * 1e-9;
            }
            // No mass action is viable (likely energy stall). Fall back to an
            // energy-building action so expansion can continue.
            if let Some(energy) =
                heuristic::energy_income_efficiency(current_economy, &completion, action, library)
            {
                return energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
        EcoDirection::BuildPower => {
            let base = match heuristic::engineer_tier(action) {
                Some(tier) => BUILD_POWER_BONUS + (tier as i32 + 1) as f64,
                None => 0.0,
            };
            base - completion.time_seconds * 1e-9
        }
        EcoDirection::Energy => {
            if let Some(energy) =
                heuristic::energy_income_efficiency(current_economy, &completion, action, library)
            {
                return ENERGY_DIRECTION_BONUS + energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
    }
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
