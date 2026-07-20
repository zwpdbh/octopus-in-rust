//! Greedy best-first scheduling algorithm.

use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use faf_blueprints::{BlueprintGraph, BlueprintLibrary, UnitKind};
use faf_solver::CompletionResult;

use crate::algorithms::SchedulingAlgorithm;
use crate::plugins::apply::ApplyPlugin;
use crate::request::EcoTarget;
use crate::result::Action;
use crate::search::{solve_action, SearchState};

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

/// Score an eco candidate by how close its simulated completion gets to the
/// target mass income.
///
/// Lower is better. Candidates that reach the target are scored primarily by
/// completion time, with a tiny penalty for overshooting mass income.
/// Candidates that do not reach the target are pushed above the simulation cap
/// so that any reaching candidate is preferred.
pub(crate) fn score_eco_candidate(
    state: &SearchState,
    action: &Action,
    library: &BlueprintLibrary,
    target: &EcoTarget,
) -> f64 {
    let Some(result) = solve_action(state, action, library) else {
        return f64::INFINITY;
    };
    let completion = result.tasks.last().cloned().unwrap_or(result.total);
    score_eco_completion(
        &completion,
        target,
        state.options.simulation_max_time_seconds,
    )
}

fn score_eco_completion(
    completion: &CompletionResult,
    target: &EcoTarget,
    max_time_seconds: f64,
) -> f64 {
    if target.is_reached(&completion.economy) {
        let mass_waste = (completion.economy.production_per_second_mass
            - target.mass_production.value())
        .max(0.0);
        return completion.time_seconds + mass_waste * 1e-6;
    }

    let mass_gap =
        (target.mass_production.value() - completion.economy.production_per_second_mass).max(0.0);
    let income = completion.economy.production_per_second_mass.max(1.0);

    max_time_seconds + mass_gap / income
}

/// Score a unit candidate by symbolic distance to the target unit.
///
/// Candidates that directly build the target use the simulated completion time;
/// all others are ranked by how many build/upgrade edges separate their result
/// from the goal.
pub(crate) fn score_unit_candidate(
    state: &SearchState,
    action: &Action,
    library: &BlueprintLibrary,
    target: &UnitKind,
) -> f64 {
    let graph = library.build_graph();
    let max_time = state.options.simulation_max_time_seconds;
    let resulting_unit = resulting_unit(action);

    if resulting_unit == *target {
        if let Some(result) = solve_action(state, action, library) {
            let completion = result.tasks.last().cloned().unwrap_or(result.total);
            completion.time_seconds
        } else {
            f64::INFINITY
        }
    } else {
        match distance_to_target(&graph, &resulting_unit, target) {
            Some(distance) => max_time + distance as f64,
            None => f64::INFINITY,
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
