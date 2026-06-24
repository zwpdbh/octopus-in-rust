//! Heuristic functions for ranking and pruning graph-growth search states.
//!
//! The functions in this module are **simplifications** used to guide the beam
//! search. They do not model FAF's discrete-project economy accurately; that
//! behavior is implemented in [`crate::economy`] and [`crate::sim`]. Instead,
//! these heuristics compute cheap completion-time estimates so the search can
//! rank candidate states without running a full simulation at every step.
//!
//! The main estimate, [`estimate_remaining_time`], treats the remaining work as
//! a continuous process with constant income and build power. At each "tick"
//! (bottleneck event) it determines the effective build rate: full build power
//! when storage can cover the drain, or the income-limited sustainable rate
//! when a resource storage is empty. This captures the interaction between
//! resource accumulation, storage burn, and build progress more accurately than
//! taking the maximum of independent estimates.
//!
//! Key assumptions:
//!
//! 1. Mass and energy income stay constant.
//! 2. Total build power stays constant.
//! 3. Remaining resource costs are spread proportionally over remaining work.
//! 4. Resources already in storage can be spent immediately.

use std::collections::HashSet;

use faf_units::{BuildTargetStats, DataIndex, Unit};

use crate::economy::EconomyState;
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
/// - The most efficient mass extractor, power generator, engineer, and factory
///   per tech tier matching the goal faction.
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

/// Estimate remaining completion time under constant income and build power.
///
/// Unlike taking the max of independent mass/energy/build estimates, this
/// models the interaction between them: resources drain continuously while
/// building, so a resource shortage can force build power to throttle even
/// before storage is empty.
///
/// Assumptions:
/// - Mass and energy income stay constant.
/// - Total build power stays constant.
/// - Remaining resource costs are distributed proportionally over remaining
///   build work (a continuous / fluid approximation).
/// - Resources already in storage can be spent immediately.
///
/// The result is exact under those assumptions. In the real planner, income and
/// build power can change, so it remains a heuristic estimate.
pub(crate) fn estimate_remaining_time(
    economy: &EconomyState,
    cost: BuildTargetStats,
    build_power: f64,
) -> f64 {
    if cost.build_time <= 0.0 {
        return 0.0;
    }
    if build_power <= 0.0 {
        return f64::INFINITY;
    }

    let mut remaining_mass = cost.build_cost_mass;
    let mut remaining_energy = cost.build_cost_energy;
    let mut remaining_work = cost.build_time;
    let mut mass_storage = economy.mass_storage;
    let mut energy_storage = economy.energy_storage;
    let mass_income = economy.net_mass_income;
    let energy_income = economy.net_energy_income;
    let mut elapsed = 0.0;

    while remaining_work > 1e-9 {
        // Cost intensity of the remaining work (fluid approximation).
        let mass_per_work = remaining_mass / remaining_work;
        let energy_per_work = remaining_energy / remaining_work;

        // Sustainable build rate for each resource (income == drain).
        let mass_sustainable_bp = if mass_per_work > 0.0 {
            mass_income / mass_per_work
        } else {
            f64::INFINITY
        };
        let energy_sustainable_bp = if energy_per_work > 0.0 {
            energy_income / energy_per_work
        } else {
            f64::INFINITY
        };

        // Effective BP is limited by resources whose storage is already empty:
        // we cannot drain them faster than income allows.
        let mut effective_bp = build_power;
        if mass_storage <= 1e-9 {
            effective_bp = effective_bp.min(mass_sustainable_bp);
        }
        if energy_storage <= 1e-9 {
            effective_bp = effective_bp.min(energy_sustainable_bp);
        }

        if effective_bp <= 1e-9 {
            return f64::INFINITY;
        }

        // Build at effective_bp. How long until a resource with positive storage
        // depletes?
        let mass_drain_rate = effective_bp * mass_per_work;
        let energy_drain_rate = effective_bp * energy_per_work;
        let net_mass = mass_income - mass_drain_rate;
        let net_energy = energy_income - energy_drain_rate;

        let time_to_finish = remaining_work / effective_bp;
        let mut dt = time_to_finish;
        if mass_storage > 1e-9 && net_mass < -1e-9 {
            dt = dt.min(-mass_storage / net_mass);
        }
        if energy_storage > 1e-9 && net_energy < -1e-9 {
            dt = dt.min(-energy_storage / net_energy);
        }

        if dt <= 1e-9 {
            // Cannot make progress; prevent an infinite loop.
            return f64::INFINITY;
        }

        elapsed += dt;
        mass_storage += net_mass * dt;
        energy_storage += net_energy * dt;
        remaining_work -= effective_bp * dt;
        remaining_mass -= mass_drain_rate * dt;
        remaining_energy -= energy_drain_rate * dt;
    }

    elapsed
}

/// Score a search state for beam ranking.
///
/// Returns an estimate of the remaining time until all `goals` are completed.
/// Lower is better.
///
/// The estimate aggregates remaining mass cost, energy cost, and build work,
/// then uses [`estimate_remaining_time`] to model how resource income, storage,
/// and build power interact while building. This is more accurate than taking
/// the max of independent estimates because it captures the case where a
/// resource shortage throttles build progress before storage is exhausted.
///
/// This is **not** how FAF's economy works in-game. In the real game, income
/// and build power change as new units are built, and projects are discrete.
/// This function instead asks: "if current income and build power stay fixed
/// and the remaining work is a continuous process, how long would it take?"
pub(crate) fn score(
    state: &GraphState,
    goals: &[&Unit],
    chain_unit_ids: &[String],
    index: &DataIndex,
) -> f64 {
    let mut total_mass = 0.0;
    let mut total_energy = 0.0;
    let mut total_work = 0.0;

    for id in chain_unit_ids {
        if has_completed_unit(state, id) {
            continue;
        }
        if let Some(unit) = index.find_unit(id) {
            if let Some(stats) = unit.build_target_stats() {
                total_mass += stats.build_cost_mass;
                total_energy += stats.build_cost_energy;
                total_work += stats.build_time;
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
            total_work += stats.build_time;
        }
    }

    let total_bp: f64 = state
        .idle_builders(index)
        .iter()
        .chain(state.active_projects.iter().flat_map(|p| p.builders.iter()))
        .map(|&b| builder_power(b, &state.graph, index))
        .sum();

    estimate_remaining_time(
        &state.economy,
        BuildTargetStats {
            build_cost_mass: total_mass,
            build_cost_energy: total_energy,
            build_time: total_work,
        },
        total_bp,
    )
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

    fn test_economy(
        mass_storage: f64,
        energy_storage: f64,
        mass_income: f64,
        energy_income: f64,
    ) -> EconomyState {
        EconomyState {
            net_mass_income: mass_income,
            net_energy_income: energy_income,
            mass_storage,
            energy_storage,
            mass_storage_cap: 0.0,
            energy_storage_cap: 0.0,
        }
    }

    #[test]
    fn estimate_build_power_bottleneck() {
        // Resources and income are abundant; only build power limits progress.
        let economy = test_economy(1000.0, 1000.0, 100.0, 100.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 100.0,
        };
        let t = estimate_remaining_time(&economy, cost, 10.0);
        assert!((t - 10.0).abs() < 1e-9, "expected 10s, got {t}");
    }

    #[test]
    fn estimate_resource_bottleneck_with_storage_burn() {
        // BP is high, income is low, and storage covers some initial work.
        // Total cost: 200 mass + 200 energy, work: 200, BP: 10.
        // Storage: 100 mass + 100 energy.
        // Income: 1 mass/s + 1 energy/s.
        // Cost intensity: 1 mass/work, 1 energy/work.
        // Sustainable BP = min(10, 1/1, 1/1) = 1.
        // Burn at BP 10: drain excess = 9 each. Storage lasts 100/9 ≈ 11.11s.
        // Work done in burn: 111.11. Remaining work: 88.89.
        // Sustainable phase: 88.89 / 1 = 88.89s.
        // Total: 100s.
        let economy = test_economy(100.0, 100.0, 1.0, 1.0);
        let cost = BuildTargetStats {
            build_cost_mass: 200.0,
            build_cost_energy: 200.0,
            build_time: 200.0,
        };
        let t = estimate_remaining_time(&economy, cost, 10.0);
        assert!((t - 100.0).abs() < 1e-6, "expected ~100s, got {t}");
    }

    #[test]
    fn estimate_no_progress_when_unaffordable() {
        // No income and no storage means we cannot finish any work.
        let economy = test_economy(0.0, 0.0, 0.0, 0.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 100.0,
        };
        let t = estimate_remaining_time(&economy, cost, 10.0);
        assert!(t.is_infinite(), "expected infinity, got {t}");
    }

    #[test]
    fn estimate_zero_work_is_instant() {
        let economy = test_economy(0.0, 0.0, 0.0, 0.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 0.0,
        };
        let t = estimate_remaining_time(&economy, cost, 10.0);
        assert!((t - 0.0).abs() < 1e-9, "expected 0s, got {t}");
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
