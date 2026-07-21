//! Greedy best-first scheduling algorithm.

use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use faf_blueprints::UnitEcoStats;
use faf_blueprints::{tech_level_of, BlueprintGraph, BlueprintLibrary, TechLevel, UnitKind};
use faf_sim_shared::EcoSnapshot;
use faf_solver::CompletionResult;

use crate::algorithms::SchedulingAlgorithm;
use crate::plugins::apply::ApplyPlugin;
use crate::request::{EcoTarget, SearchOptions};
use crate::result::Action;
use crate::search::solve_action;

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

/// Mass income threshold above which T2 tech upgrades become the top priority.
const TECH2_PRIORITY_MASS_THRESHOLD: f64 = 35.0;
/// Mass income threshold above which T3 tech upgrades become the top priority.
const TECH3_PRIORITY_MASS_THRESHOLD: f64 = 80.0;
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

/// Direction the eco scheduler should emphasize for the current step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EcoDirection {
    /// Advance tech by upgrading to the given tier.
    Tech(TechLevel),
    /// Increase mass income as efficiently as possible.
    MassIncome,
    /// Increase available build power.
    BuildPower,
    /// Increase energy income to avoid stalls.
    Energy,
}

/// Pick the direction for this scheduling step based on the current economy.
///
/// Preventing energy stalls takes precedence over everything else, because a
/// stalled economy slows or stops all construction.
pub(crate) fn choose_eco_direction(current: &EcoSnapshot, target: &EcoTarget) -> EcoDirection {
    let energy_demand = current.maintenance_consumption_per_second_energy + current.energy_drain;
    let net_energy = current.production_per_second_energy - energy_demand;

    // Prevent energy stalls before they happen by building power when the net
    // energy margin gets thin.
    if net_energy < ENERGY_BUFFER {
        return EcoDirection::Energy;
    }

    if current.production_per_second_mass >= TECH3_PRIORITY_MASS_THRESHOLD {
        return EcoDirection::Tech(TechLevel::T3);
    }
    if current.production_per_second_mass >= TECH2_PRIORITY_MASS_THRESHOLD {
        return EcoDirection::Tech(TechLevel::T2);
    }
    if !target.is_reached(current) {
        return EcoDirection::MassIncome;
    }
    EcoDirection::BuildPower
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
            if is_tech_upgrade_to(action, desired_tech) {
                // Tech upgrades beat every fallback. Faster upgrades are slightly
                // preferred among themselves.
                return TECH_PRIORITY_BONUS - completion.time_seconds * 1e-9;
            }
            // If the desired tech upgrade is stalled by energy, fall back to
            // building energy so we can tech later.
            if let Some(energy) = resource_efficiency(
                current_economy,
                &completion,
                action,
                library,
                ResourceKind::Energy,
            ) {
                return energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
        EcoDirection::MassIncome => {
            if let Some(mass) = resource_efficiency(
                current_economy,
                &completion,
                action,
                library,
                ResourceKind::Mass,
            ) {
                return MASS_DIRECTION_BONUS + mass - completion.time_seconds * 1e-9;
            }
            // No mass action is viable (likely energy stall). Fall back to an
            // energy-building action so expansion can continue.
            if let Some(energy) = resource_efficiency(
                current_economy,
                &completion,
                action,
                library,
                ResourceKind::Energy,
            ) {
                return energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
        EcoDirection::BuildPower => {
            let base = match resulting_unit(action) {
                UnitKind::Engineer(tier) => BUILD_POWER_BONUS + (tier as i32 + 1) as f64,
                _ => 0.0,
            };
            base - completion.time_seconds * 1e-9
        }
        EcoDirection::Energy => {
            if let Some(energy) = resource_efficiency(
                current_economy,
                &completion,
                action,
                library,
                ResourceKind::Energy,
            ) {
                return ENERGY_DIRECTION_BONUS + energy - completion.time_seconds * 1e-9;
            }
            0.0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Mass,
    Energy,
}

/// Compute the efficiency of an action for increasing `resource`.
///
/// Returns `Some(delta / mass_cost)` if the action increases the resource,
/// otherwise `None`.
fn resource_efficiency(
    current: &EcoSnapshot,
    completion: &CompletionResult,
    action: &Action,
    library: &BlueprintLibrary,
    resource: ResourceKind,
) -> Option<f64> {
    let resulting = resulting_unit(action);
    let mass_cost = library
        .build_cost(&resulting)
        .map(|c| c.mass)
        .unwrap_or(0.0)
        .max(0.0);
    if mass_cost <= 0.0 {
        return None;
    }

    let delta = match resource {
        ResourceKind::Mass => {
            completion.economy.production_per_second_mass.value()
                - current.production_per_second_mass.value()
        }
        ResourceKind::Energy => {
            completion.economy.production_per_second_energy.value()
                - current.production_per_second_energy.value()
        }
    };

    if delta <= 0.0 {
        return None;
    }

    Some(delta / mass_cost)
}

/// True if the action is an upgrade whose target is exactly `desired_tech`.
fn is_tech_upgrade_to(action: &Action, desired_tech: TechLevel) -> bool {
    let Action::Upgrade { to, .. } = action else {
        return false;
    };
    tech_level_of(to) == Some(desired_tech)
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
    let resulting_unit = resulting_unit(action);

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

fn resulting_unit(action: &Action) -> UnitKind {
    match action {
        Action::Build { target, .. } => target.clone(),
        Action::Upgrade { to, .. } => to.clone(),
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
