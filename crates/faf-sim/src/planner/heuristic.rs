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

use faf_units::{DataIndex, Unit};

use crate::sim::GraphState;
use crate::tech_graph::Capability;

/// Candidate units to consider building next.
///
/// This is a **pruning heuristic**: it limits the branching factor of the
/// search by only suggesting units that are likely to be useful for reaching
/// the goal. Candidates include:
///
/// - The next unbuilt unit in the prerequisite chain.
/// - The goal unit itself.
/// - The most efficient mass extractor, power generator, engineer, and factory
///   per tech tier matching the goal faction.
pub(crate) fn candidate_units<'a>(
    index: &'a DataIndex,
    state: &'a GraphState,
    goal: &Unit,
    goal_chain: &[(Capability, String)],
) -> Vec<&'a Unit> {
    let mut ids: HashSet<String> = HashSet::new();

    // Next unbuilt unit in the prerequisite chain, plus the goal itself.
    for (_, id) in goal_chain {
        if !state.has_completed_unit(id) {
            ids.insert(id.clone());
            break;
        }
    }
    ids.insert(goal.id.clone());

    let goal_faction = goal.faction();
    let faction_units: Vec<&Unit> = index
        .units
        .iter()
        .filter(|u| match goal_faction {
            Some(f) => u.is_faction(f),
            None => true,
        })
        .collect();

    // Economy and builder candidates by tier. Use efficiency (output per
    // mass invested) rather than raw cost so the planner prefers high-value
    // investments such as T1 mass extractors and T3 power generators.
    //
    // Restrict factories to land HQ factories. Air/naval factories cannot build
    // the engineers required for land experimental goals, while support
    // factories and quantum gates have different prerequisites and roles.
    let land_factories: Vec<&Unit> = faction_units
        .iter()
        .filter(|u| u.has_category("FACTORY"))
        .filter(|u| !u.has_category("AIR") && !u.has_category("NAVAL"))
        .filter(|u| !u.has_category("SUPPORTFACTORY"))
        .filter(|u| !u.has_category("GATE"))
        .copied()
        .collect();

    for tech in ["TECH1", "TECH2", "TECH3"] {
        if let Some(u) = pick_most_efficient(&faction_units, "MASSEXTRACTION", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_most_efficient(&faction_units, "ENERGYPRODUCTION", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_most_efficient(&faction_units, "ENGINEER", Some(tech)) {
            ids.insert(u.id.clone());
        }
        if let Some(u) = pick_most_efficient(&land_factories, "FACTORY", Some(tech)) {
            ids.insert(u.id.clone());
        }
    }

    ids.iter()
        .filter_map(|id| index.find_unit(id))
        .filter(|u| u.build_target_stats().is_some())
        .collect()
}

/// Pick the most efficient buildable unit matching a category and optional tech tier.
///
/// Efficiency is defined as output per mass invested:
///
/// - `MASSEXTRACTION` — mass income per second / mass cost.
/// - `ENERGYPRODUCTION` — energy income per second / mass cost.
/// - `ENGINEER` / `FACTORY` — build rate / mass cost.
///
/// This favors units that give the most economic or build-power return for the
/// mass spent, which aligns with the planner's secondary objective. In current
/// FAF data, T1 mass extractors are the most efficient mexes, while higher-tech
/// power generators and engineers are progressively more efficient.
pub(crate) fn pick_most_efficient<'a>(
    units: &[&'a Unit],
    category: &str,
    tech: Option<&str>,
) -> Option<&'a Unit> {
    units
        .iter()
        .filter(|u| u.has_category(category))
        .filter(|u| tech.is_none_or(|t| u.has_category(t)))
        .filter(|u| u.build_target_stats().is_some())
        // Hydrocarbon deposits are map-dependent and not available on every map,
        // so ignore them for general build-order planning.
        .filter(|u| !u.has_category("HYDROCARBON"))
        .max_by(|a, b| {
            let ea = efficiency(a, category).unwrap_or(0.0);
            let eb = efficiency(b, category).unwrap_or(0.0);
            ea.total_cmp(&eb)
        })
        .copied()
}

/// Compute the efficiency metric for `unit` in the given `category`.
fn efficiency(unit: &Unit, category: &str) -> Option<f64> {
    let stats = unit.build_target_stats()?;
    let cost = stats.build_cost_mass;
    if cost <= 0.0 {
        return None;
    }

    match category.to_ascii_uppercase().as_str() {
        "MASSEXTRACTION" => {
            let income = unit.economy.as_ref()?.production_per_second_mass?;
            Some(income / cost)
        }
        "ENERGYPRODUCTION" => {
            let income = unit.economy.as_ref()?.production_per_second_energy?;
            Some(income / cost)
        }
        "ENGINEER" | "FACTORY" => {
            let rate = unit.builder_capability()?.build_rate;
            Some(rate / cost)
        }
        _ => Some(1.0 / cost), // unknown category: fall back to cheapest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_units::DataIndex;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn most_efficient_mex_is_t1() {
        let index = load_index();
        let faction_units: Vec<&Unit> = index
            .units
            .iter()
            .filter(|u| u.is_faction("Cybran"))
            .collect();

        let t1 = pick_most_efficient(&faction_units, "MASSEXTRACTION", Some("TECH1"));
        let t2 = pick_most_efficient(&faction_units, "MASSEXTRACTION", Some("TECH2"));
        let t3 = pick_most_efficient(&faction_units, "MASSEXTRACTION", Some("TECH3"));

        assert_eq!(t1.unwrap().id, "URB1103", "T1 mex should be most efficient");
        assert_eq!(t2.unwrap().id, "URB1202");
        assert_eq!(t3.unwrap().id, "URB1302");
    }

    #[test]
    fn higher_tech_pgen_is_more_efficient() {
        let index = load_index();
        let faction_units: Vec<&Unit> = index
            .units
            .iter()
            .filter(|u| u.is_faction("Cybran"))
            .collect();

        let t1 = pick_most_efficient(&faction_units, "ENERGYPRODUCTION", Some("TECH1"));
        let t2 = pick_most_efficient(&faction_units, "ENERGYPRODUCTION", Some("TECH2"));
        let t3 = pick_most_efficient(&faction_units, "ENERGYPRODUCTION", Some("TECH3"));

        assert_eq!(
            t1.unwrap().id,
            "URB1101",
            "regular T1 pgen should be selected"
        );
        assert_eq!(t2.unwrap().id, "URB1201");
        assert_eq!(
            t3.unwrap().id,
            "URB1301",
            "T3 pgen should be most efficient"
        );

        // T3 pgen should be more efficient than T2 pgen.
        let t2_eff = efficiency(t2.unwrap(), "ENERGYPRODUCTION").unwrap();
        let t3_eff = efficiency(t3.unwrap(), "ENERGYPRODUCTION").unwrap();
        assert!(
            t3_eff > t2_eff,
            "T3 pgen efficiency {t3_eff} should exceed T2 {t2_eff}"
        );
    }

    #[test]
    fn higher_tech_engineer_is_more_efficient() {
        let index = load_index();
        let faction_units: Vec<&Unit> = index
            .units
            .iter()
            .filter(|u| u.is_faction("Cybran"))
            .collect();

        let t1 = pick_most_efficient(&faction_units, "ENGINEER", Some("TECH1"));
        let t2 = pick_most_efficient(&faction_units, "ENGINEER", Some("TECH2"));
        let t3 = pick_most_efficient(&faction_units, "ENGINEER", Some("TECH3"));

        assert_eq!(t1.unwrap().id, "URL0105");
        assert_eq!(t2.unwrap().id, "URL0208");
        assert_eq!(t3.unwrap().id, "URL0309");

        let t1_eff = efficiency(t1.unwrap(), "ENGINEER").unwrap();
        let t2_eff = efficiency(t2.unwrap(), "ENGINEER").unwrap();
        let t3_eff = efficiency(t3.unwrap(), "ENGINEER").unwrap();
        assert!(t2_eff > t1_eff);
        assert!(t3_eff > t2_eff);
    }
}
