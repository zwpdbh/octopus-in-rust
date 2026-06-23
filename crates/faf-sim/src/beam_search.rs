//! Beam-search build-order planner.
//!
//! The planner treats each build action as a STRIPS-like operator: from a state
//! (owned units + economy) it can build any unit whose builders are currently
//! available. It keeps the top-K most promising states each layer and returns
//! the fastest path to the goal unit.

use std::collections::HashSet;

use faf_units::{BuildTargetStats, DataIndex, Unit};

use crate::economy::{summarize_economy, total_build_power, EcoProducer, EconomyState};
use crate::planner::{PlanResult, Planner, PlannerError};
use crate::sim::BuildEvent;
use crate::tech_graph::{Capability, TechGraph};

/// Beam-search planner.
#[derive(Debug, Clone)]
pub struct BeamSearchPlanner {
    /// Number of states kept after each search layer.
    pub beam_width: usize,
    /// Maximum number of layers to explore.
    pub max_depth: usize,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
}

impl Default for BeamSearchPlanner {
    fn default() -> Self {
        Self {
            beam_width: 50,
            max_depth: 60,
            max_mex_count: 8,
            max_pgen_count: 20,
        }
    }
}

#[derive(Debug, Clone)]
struct SearchState {
    /// Blueprint ids currently owned, sorted. Duplicates are allowed.
    owned: Vec<String>,
    /// Current economy state.
    economy: EconomyState,
    /// Elapsed game time in seconds.
    elapsed: f64,
    /// Total build power available from owned units.
    build_rate: f64,
    /// Completed build events leading to this state.
    events: Vec<BuildEvent>,
}

impl BeamSearchPlanner {
    /// Return a sorted list of unit ids currently owned.
    fn owned_set(owned: &[String]) -> HashSet<&str> {
        owned.iter().map(|s| s.as_str()).collect()
    }

    /// Build `unit` from `state`, returning the resulting state.
    fn build_unit(
        &self,
        index: &DataIndex,
        state: &SearchState,
        unit: &Unit,
    ) -> Option<SearchState> {
        let stats = unit.build_target_stats()?;

        let mass_dt = optimistic_time(
            stats.build_cost_mass,
            state.economy.mass_storage,
            state.economy.net_mass_income,
        );
        let energy_dt = optimistic_time(
            stats.build_cost_energy,
            state.economy.energy_storage,
            state.economy.net_energy_income,
        );
        let build_dt = if state.build_rate > 0.0 {
            stats.build_time / state.build_rate
        } else {
            f64::INFINITY
        };
        let dt = mass_dt.max(energy_dt).max(build_dt);
        if dt.is_infinite() {
            return None;
        }

        let mut new_owned = state.owned.clone();
        new_owned.push(unit.id.clone());
        new_owned.sort();
        let new_owned_refs: Vec<&Unit> = resolve_units(index, &new_owned)?;

        let net_flow = summarize_economy(&new_owned_refs, &[]);
        let (mass_cap, energy_cap) = storage_caps(&new_owned_refs);

        let mut mass_storage = (state.economy.mass_storage + state.economy.net_mass_income * dt
            - stats.build_cost_mass)
            .min(state.economy.mass_storage_cap)
            .max(0.0);
        let mut energy_storage = (state.economy.energy_storage
            + state.economy.net_energy_income * dt
            - stats.build_cost_energy)
            .min(state.economy.energy_storage_cap)
            .max(0.0);

        mass_storage = mass_storage.min(mass_cap).max(0.0);
        energy_storage = energy_storage.min(energy_cap).max(0.0);

        let mut events = state.events.clone();
        events.push(BuildEvent {
            time: state.elapsed + dt,
            unit_id: unit.id.clone(),
            unit_name: unit.display_name(),
        });

        Some(SearchState {
            owned: new_owned,
            economy: EconomyState {
                net_mass_income: net_flow.mass_per_second,
                net_energy_income: net_flow.energy_per_second,
                mass_storage,
                energy_storage,
                mass_storage_cap: mass_cap,
                energy_storage_cap: energy_cap,
            },
            elapsed: state.elapsed + dt,
            build_rate: total_build_power(&new_owned_refs).0,
            events,
        })
    }

    /// All candidate units that might appear in a plan for `goal`.
    fn candidate_units<'a>(
        &self,
        index: &'a DataIndex,
        graph: &'a TechGraph<'a>,
        goal: &'a Unit,
        chain_units: &[String],
    ) -> Vec<&'a Unit> {
        let mut ids: HashSet<&str> = HashSet::new();

        if let Ok(prereqs) = graph.all_prerequisites_default(&goal.id) {
            for u in prereqs {
                ids.insert(&u.id);
            }
        }
        for id in chain_units {
            ids.insert(id);
        }
        ids.insert(&goal.id);

        let goal_faction = goal.faction();
        let mut candidates: Vec<&Unit> = index
            .units
            .iter()
            .filter(|u| match goal_faction {
                Some(f) => u.is_faction(f),
                None => true,
            })
            .filter(|u| {
                ids.contains(u.id.as_str())
                    || is_economy_unit(u)
                    || u.builder_capability().is_some()
            })
            .collect();

        candidates.sort_by(|a, b| a.id.cmp(&b.id));
        candidates.dedup_by(|a, b| a.id.eq_ignore_ascii_case(&b.id));
        candidates
    }

    /// Admissible-ish heuristic estimate: time to build the remaining units in
    /// the capability chain (which includes the goal), from the current economy.
    fn score(&self, state: &SearchState, chain: &[(String, BuildTargetStats)]) -> f64 {
        let (total_mass, total_energy, total_build_time) = chain
            .iter()
            .filter(|(id, _)| !state.owned.iter().any(|o| o.eq_ignore_ascii_case(id)))
            .fold((0.0, 0.0, 0.0), |(m, e, t), (_, stats)| {
                (
                    m + stats.build_cost_mass,
                    e + stats.build_cost_energy,
                    t + stats.build_time,
                )
            });

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
        let build_time = if state.build_rate > 0.0 {
            total_build_time / state.build_rate
        } else {
            f64::INFINITY
        };

        mass_time.max(energy_time).max(build_time)
    }

    /// Generate successor states by building currently buildable candidates.
    ///
    /// To keep branching manageable while still guaranteeing progress, we only
    /// consider:
    /// - the next unbuilt unit on the capability chain,
    /// - economy units (mex/pgen), up to caps,
    /// - additional builders (engineers/factories).
    fn successors(
        &self,
        index: &DataIndex,
        graph: &TechGraph,
        state: &SearchState,
        candidates: &[&Unit],
        chain_unit_ids: &[String],
    ) -> Vec<SearchState> {
        let owned_membership = Self::owned_set(&state.owned);
        let owned_refs: Vec<&Unit> = state
            .owned
            .iter()
            .filter_map(|id| index.find_unit(id))
            .collect();
        let mex_count = owned_refs
            .iter()
            .filter(|u| u.has_category("MASSEXTRACTION"))
            .count();
        let pgen_count = owned_refs
            .iter()
            .filter(|u| u.has_category("ENERGYPRODUCTION"))
            .count();

        let next_chain_id = chain_unit_ids
            .iter()
            .find(|id| !state.owned.iter().any(|o| o.eq_ignore_ascii_case(id)))
            .map(|s| s.as_str());

        let mut successors = Vec::new();

        for unit in candidates {
            let is_next_chain = next_chain_id.map_or(false, |id| unit.id.eq_ignore_ascii_case(id));
            let is_economy = is_economy_unit(unit);
            if is_economy {
                if unit.has_category("MASSEXTRACTION") && mex_count >= self.max_mex_count {
                    continue;
                }
                if unit.has_category("ENERGYPRODUCTION") && pgen_count >= self.max_pgen_count {
                    continue;
                }
            }
            if !(is_next_chain || is_economy || unit.builder_capability().is_some()) {
                continue;
            }

            let builders = match graph.builders_for(&unit.id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if !builders
                .iter()
                .any(|b| owned_membership.contains(b.id.as_str()))
            {
                continue;
            }
            if let Some(next) = self.build_unit(index, state, unit) {
                successors.push(next);
            }
        }

        successors
    }

    /// True if the goal unit is owned in this state.
    fn goal_reached(&self, state: &SearchState, goal: &Unit) -> bool {
        state
            .owned
            .iter()
            .any(|id| id.eq_ignore_ascii_case(&goal.id))
    }

    /// Convert a winning search state into a plan result.
    fn to_plan_result(&self, state: SearchState) -> PlanResult {
        PlanResult {
            completion_time: state.elapsed,
            final_economy: state.economy,
            events: state.events,
        }
    }
}

impl Planner for BeamSearchPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        graph: &TechGraph,
        starting_units: &[&Unit],
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        let starting_refs: Vec<&Unit> = starting_units.to_vec();
        let initial_flow = summarize_economy(&starting_refs, &[]);
        let (mass_cap, energy_cap) = storage_caps(&starting_refs);

        let initial = SearchState {
            owned: starting_refs.iter().map(|u| u.id.clone()).collect(),
            economy: EconomyState {
                net_mass_income: initial_flow.mass_per_second,
                net_energy_income: initial_flow.energy_per_second,
                mass_storage: mass_cap,
                energy_storage: energy_cap,
                mass_storage_cap: mass_cap,
                energy_storage_cap: energy_cap,
            },
            elapsed: 0.0,
            build_rate: total_build_power(&starting_refs).0,
            events: Vec::new(),
        };

        let chain = graph.prerequisite_chain(&goal.id, Capability::ACU)?;
        let chain_unit_ids: Vec<String> = chain.iter().map(|(_, id)| id.clone()).collect();
        let chain_stats: Vec<(String, BuildTargetStats)> = chain
            .into_iter()
            .filter_map(|(_, id)| {
                let unit = index.find_unit(&id)?;
                let stats = unit.build_target_stats()?;
                Some((id, stats))
            })
            .collect();

        let candidates = self.candidate_units(index, graph, goal, &chain_unit_ids);
        let mut beam = vec![initial];
        let mut visited: HashSet<Vec<String>> = HashSet::new();

        for _ in 0..self.max_depth {
            let mut candidates_next: Vec<SearchState> = Vec::new();

            for state in beam {
                if self.goal_reached(&state, goal) {
                    return Ok(self.to_plan_result(state));
                }
                if !visited.insert(state.owned.clone()) {
                    continue;
                }
                candidates_next.extend(self.successors(
                    index,
                    graph,
                    &state,
                    &candidates,
                    &chain_unit_ids,
                ));
            }

            candidates_next.sort_by(|a, b| {
                let fa = a.elapsed + self.score(a, &chain_stats);
                let fb = b.elapsed + self.score(b, &chain_stats);
                fa.total_cmp(&fb)
            });

            beam = candidates_next.into_iter().take(self.beam_width).collect();
            if beam.is_empty() {
                break;
            }
        }

        // Final pass: any remaining state may already satisfy the goal.
        for state in beam {
            if self.goal_reached(&state, goal) {
                return Ok(self.to_plan_result(state));
            }
        }

        Err(PlannerError::SearchExhausted)
    }
}

/// Optimistic time needed to afford `cost` given current `storage` and `income`.
fn optimistic_time(cost: f64, storage: f64, income: f64) -> f64 {
    if cost <= storage {
        0.0
    } else if income > 0.0 {
        (cost - storage) / income
    } else {
        f64::INFINITY
    }
}

fn resolve_units<'a>(index: &'a DataIndex, ids: &[String]) -> Option<Vec<&'a Unit>> {
    let mut units = Vec::with_capacity(ids.len());
    for id in ids {
        units.push(index.find_unit(id)?);
    }
    Some(units)
}

fn storage_caps(units: &[&Unit]) -> (f64, f64) {
    let mut mass = 0.0;
    let mut energy = 0.0;
    for u in units {
        if let Some(e) = &u.economy {
            mass += e.storage_mass.unwrap_or(0.0);
            energy += e.storage_energy.unwrap_or(0.0);
        }
    }
    (mass, energy)
}

fn is_economy_unit(unit: &Unit) -> bool {
    let production = EcoProducer::production(unit);
    unit.has_category("MASSEXTRACTION")
        || unit.has_category("ENERGYPRODUCTION")
        || production.mass_per_second > 0.0
        || production.energy_per_second > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use crate::planner::Strategy;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn beam_finds_pgen_from_acu() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = BeamSearchPlanner {
            beam_width: 5,
            max_depth: 5,
            ..Default::default()
        };
        let result = planner.plan(&index, &graph, &[acu], goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URB1101")),
            "plan should build the goal pgen"
        );
        assert!(result.completion_time > 0.0);
    }

    #[test]
    fn beam_finds_monkeylord() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URL0402").expect("Monkeylord exists");

        let planner = BeamSearchPlanner {
            beam_width: 100,
            max_depth: 80,
            ..Default::default()
        };
        let result = planner.plan(&index, &graph, &[acu], goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URL0402")),
            "plan should reach the Monkeylord"
        );
        assert!(result.completion_time > 0.0);
        assert!(
            result.completion_time < 9000.0,
            "beam search should beat the 147-minute greedy baseline"
        );
    }

    #[test]
    fn strategy_parses_beam() {
        assert_eq!(Strategy::from_str("beam").unwrap(), Strategy::Beam);
    }
}
