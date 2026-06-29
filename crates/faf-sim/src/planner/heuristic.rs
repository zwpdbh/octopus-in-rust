//! Heuristic functions for ranking and pruning graph-growth search states.
//!
//! The functions in this module are **simplifications** used to guide the
//! planner's search. They do not model FAF's discrete-project economy
//! accurately; that behavior is implemented in [`crate::economy`] and
//! [`crate::sim`]. Instead, these heuristics compute cheap completion-time
//! estimates so the search can rank candidate states without running a full
//! simulation at every step.
//!
//! The main estimate lives on [`crate::economy::EconomyState`] as
//! [`estimate_remaining_time`](crate::economy::EconomyState::estimate_remaining_time).
//! It treats the remaining work as a continuous process with constant income and
//! build power. At each "tick" (bottleneck event) it determines the effective
//! build rate: full build power when storage can cover the drain, or the
//! income-limited sustainable rate when a resource storage is empty. This
//! captures the interaction between resource accumulation, storage burn, and
//! build progress more accurately than taking the maximum of independent
//! estimates.
//!
//! The state-level wrapper
//! [`GraphState::estimate_remaining_time_to_goal`](crate::sim::GraphState::estimate_remaining_time_to_goal)
//! aggregates the remaining work for a goal and its prerequisites, then calls
//! the economy estimate.
//!
//! Key assumptions:
//!
//! 1. Mass and energy income stay constant.
//! 2. Total build power stays constant.
//! 3. Remaining resource costs are spread proportionally over remaining work.
//! 4. Resources already in storage can be spent immediately.
//!
//! This module is temporarily unused while the MCTS planner is being
//! implemented.
#![allow(dead_code)]

use std::collections::HashSet;

use crate::sim::adjacency::production_multiplier;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitDef, UnitKind, Units};

/// Candidate units to consider building next.
///
/// This is a **pruning heuristic**: it limits the branching factor of the
/// search by only suggesting units that are likely to be useful for reaching
/// the goal. Candidates come from two explicit rules:
///
/// 1. [`add_goal_path_candidates`]: the next missing prerequisite and the goal itself.
/// 2. [`add_efficient_economy_candidates`]: the most efficient eco/builder unit
///    of each category per tech tier.
pub(crate) fn candidate_units(
    units: &Units,
    state: &GraphState,
    goal_id: &UnitKind,
    goal_chain: &[UnitKind],
) -> Vec<UnitKind> {
    let mut ids: HashSet<UnitKind> = HashSet::new();

    add_goal_path_candidates(&mut ids, state, goal_id, goal_chain);
    add_efficient_economy_candidates(&mut ids, units);

    ids.into_iter().collect()
}

/// Rule 1: add the next unbuilt unit on the path to the goal, plus the goal itself.
///
/// The prerequisite chain tells us what must exist before the goal can be built.
/// We only add the *first* missing step so the planner can walk the chain one
/// link at a time, but we always add the goal so the search can attempt the
/// final target as soon as it becomes legal.
fn add_goal_path_candidates(
    ids: &mut HashSet<UnitKind>,
    state: &GraphState,
    goal_id: &UnitKind,
    goal_chain: &[UnitKind],
) {
    for id in goal_chain {
        if !state.has_completed_unit(id) {
            ids.insert(id.clone());
            break;
        }
    }
    ids.insert(goal_id.clone());
}

/// Rule 2: add the most efficient eco/builder unit for each category and tier.
///
/// For every tech tier we consider four categories: mass extractor, power
/// generator, engineer, and factory.  Within each category we keep only the
/// unit with the highest output (or build rate) per mass invested.  This
/// encodes the heuristic that investing in strong economy and builder capacity
/// tends to produce faster build orders.
fn add_efficient_economy_candidates(ids: &mut HashSet<UnitKind>, units: &Units) {
    for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
        for category in [
            UnitKind::Mex(tech),
            UnitKind::Pgen(tech),
            UnitKind::Engineer(tech),
            UnitKind::Factory(tech),
        ] {
            if let Some(k) = pick_most_efficient(units, tech, category) {
                ids.insert(k);
            }
        }
    }
}

/// Pick the most efficient buildable unit matching a category and tech tier.
///
/// `category` determines both the variant to match and the efficiency formula.
///
/// Efficiency is defined as output per mass invested:
///
/// - `Mex` — mass income per second / mass cost.
/// - `Pgen` — energy income per second / mass cost.
/// - `Engineer` / `Factory` — build rate / mass cost.
pub(crate) fn pick_most_efficient(
    units: &Units,
    tech: TechLevel,
    category: UnitKind,
) -> Option<UnitKind> {
    units
        .defs()
        .values()
        .filter(|d| kind_tech(&d.kind) == Some(tech))
        .filter(|d| kind_matches_category(&d.kind, &category))
        .filter(|d| d.cost.mass > 0.0)
        .max_by(|a, b| efficiency(a, &category).total_cmp(&efficiency(b, &category)))
        .map(|d| d.kind.clone())
}

/// Return the tech level of common unit kinds.
fn kind_tech(kind: &UnitKind) -> Option<TechLevel> {
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => {
            Some(*t)
        }
        UnitKind::CapT2Mex => Some(TechLevel::T2),
        UnitKind::CapT3Mex => Some(TechLevel::T3),
        UnitKind::EnergyStorage => Some(TechLevel::T1),
        _ => None,
    }
}

/// True if `kind` belongs to the same broad category as `category`.
fn kind_matches_category(kind: &UnitKind, category: &UnitKind) -> bool {
    matches!(
        (kind, category),
        (UnitKind::Mex(_), UnitKind::Mex(_))
            | (UnitKind::Pgen(_), UnitKind::Pgen(_))
            | (UnitKind::Engineer(_), UnitKind::Engineer(_))
            | (UnitKind::Factory(_), UnitKind::Factory(_))
            | (UnitKind::EnergyStorage, UnitKind::EnergyStorage)
            | (UnitKind::CapT2Mex, UnitKind::Mex(_))
            | (UnitKind::CapT3Mex, UnitKind::Mex(_))
    )
}

/// Compute the efficiency metric for `def` in the given category.
fn efficiency(def: &UnitDef, category: &UnitKind) -> f64 {
    let mass_cost = def.cost.mass;
    if mass_cost <= 0.0 {
        return 0.0;
    }

    match category {
        UnitKind::Mex(_) => def.mass_income / mass_cost,
        UnitKind::Pgen(_) => def.energy_income / mass_cost,
        UnitKind::Engineer(_) | UnitKind::Factory(_) => def.build_rate / mass_cost,
        UnitKind::CapT2Mex | UnitKind::CapT3Mex => {
            // Capped mexes receive the max mass-storage adjacency bonus (+50%).
            (def.mass_income * production_multiplier(4)) / mass_cost
        }
        UnitKind::EnergyStorage => 0.0,
        _ => 1.0 / mass_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Units;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn most_efficient_mex_is_tier_appropriate() {
        let units = load_units();

        let t1 = pick_most_efficient(&units, TechLevel::T1, UnitKind::Mex(TechLevel::T1));
        let t2 = pick_most_efficient(&units, TechLevel::T2, UnitKind::Mex(TechLevel::T2));
        let t3 = pick_most_efficient(&units, TechLevel::T3, UnitKind::Mex(TechLevel::T3));

        assert_eq!(t1, Some(UnitKind::Mex(TechLevel::T1)));
        // Capped mexes are the most efficient mex-like investment at T2/T3.
        assert_eq!(t2, Some(UnitKind::CapT2Mex));
        assert_eq!(t3, Some(UnitKind::CapT3Mex));
    }

    #[test]
    fn higher_tech_pgen_is_more_efficient() {
        let units = load_units();

        let t1 = pick_most_efficient(&units, TechLevel::T1, UnitKind::Pgen(TechLevel::T1));
        let t2 = pick_most_efficient(&units, TechLevel::T2, UnitKind::Pgen(TechLevel::T2));
        let t3 = pick_most_efficient(&units, TechLevel::T3, UnitKind::Pgen(TechLevel::T3));

        assert_eq!(t1, Some(UnitKind::Pgen(TechLevel::T1)));
        assert_eq!(t2, Some(UnitKind::Pgen(TechLevel::T2)));
        assert_eq!(t3, Some(UnitKind::Pgen(TechLevel::T3)));

        // T3 pgen should be more efficient than T2 pgen.
        let t2_def = units.def(&UnitKind::Pgen(TechLevel::T2)).unwrap();
        let t3_def = units.def(&UnitKind::Pgen(TechLevel::T3)).unwrap();
        let t2_eff = efficiency(t2_def, &UnitKind::Pgen(TechLevel::T2));
        let t3_eff = efficiency(t3_def, &UnitKind::Pgen(TechLevel::T3));
        assert!(
            t3_eff > t2_eff,
            "T3 pgen efficiency {t3_eff} should exceed T2 {t2_eff}"
        );
    }

    #[test]
    fn higher_tech_engineer_is_more_efficient() {
        let units = load_units();

        let t1 = pick_most_efficient(&units, TechLevel::T1, UnitKind::Engineer(TechLevel::T1));
        let t2 = pick_most_efficient(&units, TechLevel::T2, UnitKind::Engineer(TechLevel::T2));
        let t3 = pick_most_efficient(&units, TechLevel::T3, UnitKind::Engineer(TechLevel::T3));

        assert_eq!(t1, Some(UnitKind::Engineer(TechLevel::T1)));
        assert_eq!(t2, Some(UnitKind::Engineer(TechLevel::T2)));
        assert_eq!(t3, Some(UnitKind::Engineer(TechLevel::T3)));

        let t1_def = units.def(&UnitKind::Engineer(TechLevel::T1)).unwrap();
        let t2_def = units.def(&UnitKind::Engineer(TechLevel::T2)).unwrap();
        let t3_def = units.def(&UnitKind::Engineer(TechLevel::T3)).unwrap();
        let t1_eff = efficiency(t1_def, &UnitKind::Engineer(TechLevel::T1));
        let t2_eff = efficiency(t2_def, &UnitKind::Engineer(TechLevel::T2));
        let t3_eff = efficiency(t3_def, &UnitKind::Engineer(TechLevel::T3));
        assert!(t2_eff > t1_eff);
        assert!(t3_eff > t2_eff);
    }
}
