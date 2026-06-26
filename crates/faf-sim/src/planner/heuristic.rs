//! Heuristic functions for ranking and pruning graph-growth search states.
//!
//! The functions in this module are **simplifications** used to guide the beam
//! search. They do not model FAF's discrete-project economy accurately; that
//! behavior is implemented in [`crate::economy`] and [`crate::sim`]. Instead,
//! these heuristics compute cheap completion-time estimates so the search can
//! rank candidate states without running a full simulation at every step.
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

use std::collections::HashSet;

use crate::sim::GraphState;
use crate::units::{TechLevel, UnitDef, UnitKind, Units};

/// Candidate units to consider building next.
///
/// This is a **pruning heuristic**: it limits the branching factor of the
/// search by only suggesting units that are likely to be useful for reaching
/// the goal. Candidates include:
///
/// - The next unbuilt unit in the prerequisite chain.
/// - The goal unit itself.
/// - The most efficient mass extractor, power generator, engineer, and factory
///   per tech tier.
pub(crate) fn candidate_units(
    units: &Units,
    state: &GraphState,
    goal_id: &UnitKind,
    goal_chain: &[UnitKind],
) -> Vec<UnitKind> {
    let mut ids: HashSet<UnitKind> = HashSet::new();

    // Next unbuilt unit in the prerequisite chain, plus the goal itself.
    for id in goal_chain {
        if !state.has_completed_unit(id) {
            ids.insert(id.clone());
            break;
        }
    }
    ids.insert(goal_id.clone());

    // Economy and builder candidates by tier. Use efficiency (output per
    // mass invested) rather than raw cost so the planner prefers high-value
    // investments such as T1 mass extractors and T3 power generators.
    for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
        if let Some(k) = pick_most_efficient(units, tech, UnitKind::Mex(tech)) {
            ids.insert(k);
        }
        if let Some(k) = pick_most_efficient(units, tech, UnitKind::Pgen(tech)) {
            ids.insert(k);
        }
        if let Some(k) = pick_most_efficient(units, tech, UnitKind::Engineer(tech)) {
            ids.insert(k);
        }
        if let Some(k) = pick_most_efficient(units, tech, UnitKind::Factory(tech)) {
            ids.insert(k);
        }
    }

    ids.into_iter().collect()
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
        assert_eq!(t2, Some(UnitKind::Mex(TechLevel::T2)));
        assert_eq!(t3, Some(UnitKind::Mex(TechLevel::T3)));
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
