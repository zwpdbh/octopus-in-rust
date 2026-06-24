//! Heuristic functions for ranking and pruning graph-growth search states.
//!
//! The functions in this module are **optimistic simplifications** used to guide
//! the beam search. They do **not** model FAF's continuous-drain economy
//! accurately; that behavior is implemented in [`crate::economy`] and
//! [`crate::sim`]. Instead, these heuristics compute cheap lower-bound estimates
//! so the search can rank candidate states without running a full simulation at
//! every step.
//!
//! Key assumptions that make these estimates optimistic:
//!
//! 1. All future income can be directed at the remaining goals.
//! 2. There are no competing drains from other projects.
//! 3. Build power can be freely allocated to the remaining work.
//!
//! Because the estimates are lower bounds, a state with a lower score is at
//! least as promising in the best case. The beam search explores the most
//! promising states first.

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::planner::search::has_completed_unit;
use crate::sim::{builder_power, GraphState};
use crate::tech_graph::Capability;

/// Candidate units to consider building next.
///
/// This is a **pruning heuristic**: it limits the branching factor of the
/// search by only suggesting units that are likely to be useful for reaching
/// the goals. Candidates include:
///
/// - The next unbuilt unit in each prerequisite chain.
/// - The goal units themselves.
/// - The cheapest mass extractor, power generator, engineer, and factory per
///   tech tier matching the goal faction.
pub(crate) fn candidate_units<'a>(
    index: &'a DataIndex,
    state: &'a GraphState,
    goals: &[&Unit],
    goal_chains: &[Vec<(Capability, String)>],
) -> Vec<&'a Unit> {
    let mut ids: HashSet<String> = HashSet::new();

    // Next unbuilt unit in each prerequisite chain, plus the goal itself.
    for chain in goal_chains {
        for (_, id) in chain {
            if !has_completed_unit(state, id) {
                ids.insert(id.clone());
                break;
            }
        }
    }
    for goal in goals {
        ids.insert(goal.id.clone());
    }

    let goal_faction = goals.first().and_then(|g| g.faction());
    let faction_units: Vec<&Unit> = index
        .units
        .iter()
        .filter(|u| match goal_faction {
            Some(f) => u.is_faction(f),
            None => true,
        })
        .collect();

    // Economy and builder candidates by tier.
    for tech in ["TECH1", "TECH2", "TECH3"] {
        if let Some(u) = pick_cheapest(&faction_units, "MASSEXTRACTION", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_cheapest(&faction_units, "ENERGYPRODUCTION", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_cheapest(&faction_units, "ENGINEER", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_cheapest(&faction_units, "FACTORY", Some(tech)) {
            ids.insert(u.id.clone());
        }
    }

    ids.iter()
        .filter_map(|id| index.find_unit(id))
        .filter(|u| u.build_target_stats().is_some())
        .collect()
}

/// Score a search state for beam ranking.
///
/// Returns an optimistic estimate of the remaining time until all `goals` are
/// completed. Lower is better.
///
/// The estimate is the maximum of three optimistic lower bounds:
///
/// 1. **Mass time** — time to accumulate remaining mass costs at current mass
///    income.
/// 2. **Energy time** — time to accumulate remaining energy costs at current
///    energy income.
/// 3. **Build time** — time to complete remaining build work at current total
///    build power.
///
/// This is **not** how FAF's economy works in-game. In the real game, projects
/// drain resources continuously and progress is scaled by the most-constrained
/// resource. This function instead asks: "if all income and build power could
/// be dedicated to the remaining goals, how long would it take?" That gives an
/// admissible lower bound useful for ranking states.
pub(crate) fn score(
    state: &GraphState,
    goals: &[&Unit],
    chain_unit_ids: &[String],
    index: &DataIndex,
) -> f64 {
    let mut total_mass = 0.0;
    let mut total_energy = 0.0;
    let mut total_build_time = 0.0;

    for id in chain_unit_ids {
        if has_completed_unit(state, id) {
            continue;
        }
        if let Some(unit) = index.find_unit(id) {
            if let Some(stats) = unit.build_target_stats() {
                total_mass += stats.build_cost_mass;
                total_energy += stats.build_cost_energy;
                total_build_time += stats.build_time;
            }
        }
    }

    for goal in goals {
        if has_completed_unit(state, &goal.id) {
            continue;
        }
        if let Some(stats) = goal.build_target_stats() {
            total_mass += stats.build_cost_mass;
            total_energy += stats.build_cost_energy;
            total_build_time += stats.build_time;
        }
    }

    let mass_time = optimistic_time(
        total_mass,
        state.economy.mass_storage,
        state.economy.net_mass_income,
    );
    let energy_time = optimistic_time(
        total_energy,
        state.economy.energy_storage,
        state.economy.net_energy_income,
    );

    let total_bp: f64 = state
        .idle_builders
        .iter()
        .chain(state.active_projects.iter().flat_map(|p| p.builders.iter()))
        .map(|&b| builder_power(b, &state.graph, index))
        .sum();
    let build_time = if total_bp > 0.0 {
        total_build_time / total_bp
    } else {
        f64::INFINITY
    };

    mass_time.max(energy_time).max(build_time)
}

/// Pick the cheapest buildable unit matching a category and optional tech tier.
///
/// "Cheapest" is defined by blueprint mass cost. This is a simple heuristic
/// used by [`candidate_units`] to suggest economy and builder units.
pub(crate) fn pick_cheapest<'a>(
    units: &[&'a Unit],
    category: &str,
    tech: Option<&str>,
) -> Option<&'a Unit> {
    units
        .iter()
        .filter(|u| u.has_category(category))
        .filter(|u| tech.is_none_or(|t| u.has_category(t)))
        .filter(|u| u.build_target_stats().is_some())
        .min_by(|a, b| {
            let ca = a.build_target_stats().unwrap().build_cost_mass;
            let cb = b.build_target_stats().unwrap().build_cost_mass;
            ca.total_cmp(&cb)
        })
        .copied()
}

/// Optimistic time needed to afford `cost` given current `storage` and `income`.
///
/// This is a **lower bound**, not a realistic FAF forecast. In the real game
/// you can start projects before having the full cost; progress is scaled by
/// the most-constrained resource. This function instead asks: "if all income
/// went toward this cost, how long until storage covers it?"
///
/// Returns:
/// - `0.0` if `cost <= storage` (already affordable).
/// - `(cost - storage) / income` if `income > 0.0`.
/// - `f64::INFINITY` otherwise (unaffordable with non-positive income).
pub(crate) fn optimistic_time(cost: f64, storage: f64, income: f64) -> f64 {
    if cost <= storage {
        0.0
    } else if income > 0.0 {
        (cost - storage) / income
    } else {
        f64::INFINITY
    }
}
