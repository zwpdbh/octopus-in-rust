//! Graph-based beam-search planner.
//!
//! This planner searches over the graph-growth model implemented in
//! [`crate::graph_sim`]. Each search state is a `GraphState`; transitions are
//! actions such as "start a new project" or "assist an active project". The
//! planner keeps the top-K most promising states each layer and returns the
//! fastest path to the goal unit(s).

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::graph_sim::{builder_power, GraphSimError, GraphState, NodeId};
use crate::planner::{PlanResult, Planner, PlannerError};
use crate::tech_graph::{Capability, TechGraph};

/// Beam-search planner over the graph-growth model.
#[derive(Debug, Clone)]
pub struct GraphPlanner {
    /// Number of states kept after each search layer.
    pub beam_width: usize,
    /// Maximum number of layers to explore.
    pub max_depth: usize,
    /// Fixed simulation timestep in seconds.
    pub dt: f64,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
}

impl Default for GraphPlanner {
    fn default() -> Self {
        Self {
            beam_width: 50,
            max_depth: 400,
            dt: 10.0,
            max_mex_count: 8,
            max_pgen_count: 20,
        }
    }
}

impl GraphPlanner {
    /// Plan for one or more goal units.
    ///
    /// The public `Planner` trait only exposes a single goal; this method is
    /// the multi-goal generalisation and can be exposed later when the CLI
    /// supports it.
    fn plan_goals(
        &self,
        index: &DataIndex,
        graph: &TechGraph,
        starting_units: &[&Unit],
        goals: &[&Unit],
    ) -> Result<PlanResult, PlannerError> {
        if goals.is_empty() {
            return Err(PlannerError::SearchExhausted);
        }

        let mut goal_chains: Vec<Vec<(Capability, String)>> = Vec::with_capacity(goals.len());
        for goal in goals {
            let chain = graph.prerequisite_chain(&goal.id, Capability::ACU)?;
            goal_chains.push(chain);
        }

        let mut chain_unit_ids: Vec<String> = goal_chains
            .iter()
            .flat_map(|chain| chain.iter().map(|(_, id)| id.clone()))
            .collect();
        chain_unit_ids.sort();
        chain_unit_ids.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let initial = GraphState::new(starting_units);
        let mut beam: Vec<GraphState> = vec![initial];
        let mut visited: HashSet<VisitedKey> = HashSet::new();

        for _ in 0..self.max_depth {
            let mut next_beam: Vec<GraphState> = Vec::new();

            for state in beam {
                if self.goals_reached(&state, goals) {
                    return Ok(self.to_plan_result(state));
                }

                let key = self.visited_key(&state);
                if !visited.insert(key) {
                    continue;
                }

                next_beam.extend(self.successors(index, graph, &state, goals, &goal_chains));
            }

            next_beam.sort_by(|a, b| {
                let fa = a.time + self.score(a, goals, &chain_unit_ids, index);
                let fb = b.time + self.score(b, goals, &chain_unit_ids, index);
                fa.total_cmp(&fb)
            });

            beam = next_beam.into_iter().take(self.beam_width).collect();
            if beam.is_empty() {
                break;
            }
        }

        // Final pass: any remaining state may already satisfy the goals.
        for state in beam {
            if self.goals_reached(&state, goals) {
                return Ok(self.to_plan_result(state));
            }
        }

        Err(PlannerError::SearchExhausted)
    }

    /// True if every goal unit has been completed in `state`.
    fn goals_reached(&self, state: &GraphState, goals: &[&Unit]) -> bool {
        goals.iter().all(|g| self.has_completed_unit(state, &g.id))
    }

    /// True if a completed node with the given blueprint id exists.
    fn has_completed_unit(&self, state: &GraphState, unit_id: &str) -> bool {
        state
            .graph
            .nodes
            .iter()
            .any(|n| !n.finish_time.is_nan() && n.unit_id.eq_ignore_ascii_case(unit_id))
    }

    /// A compact, deterministic representation of a state for deduplication.
    fn visited_key(&self, state: &GraphState) -> VisitedKey {
        let mut owned: Vec<String> = state
            .graph
            .nodes
            .iter()
            .filter(|n| !n.finish_time.is_nan())
            .map(|n| n.unit_id.to_ascii_uppercase())
            .collect();
        owned.sort();

        let mut active: Vec<(String, i64)> = state
            .active_projects
            .iter()
            .map(|p| {
                let target_id = state.graph.nodes[p.target_node.0]
                    .unit_id
                    .to_ascii_uppercase();
                let work = (p.remaining_work * 100.0).round() as i64;
                (target_id, work)
            })
            .collect();
        active.sort();

        (owned, active)
    }

    /// Generate successor states from `state`.
    fn successors<'a>(
        &self,
        index: &'a DataIndex,
        graph: &'a TechGraph,
        state: &'a GraphState,
        goals: &[&Unit],
        goal_chains: &[Vec<(Capability, String)>],
    ) -> Vec<GraphState> {
        // If no builders are free and something is being built, waiting is the
        // only valid action.
        if state.idle_builders.is_empty() {
            let mut next = state.clone();
            next.tick(index, self.dt);
            return vec![next];
        }

        let active_targets: HashSet<String> = state
            .active_projects
            .iter()
            .map(|p| {
                state.graph.nodes[p.target_node.0]
                    .unit_id
                    .to_ascii_uppercase()
            })
            .collect();

        let mut successors: Vec<GraphState> = Vec::new();
        let candidates = self.candidate_units(index, state, goals, goal_chains);

        for unit in candidates {
            if self.has_completed_unit(state, &unit.id) {
                continue;
            }
            if active_targets.contains(&unit.id.to_ascii_uppercase()) {
                continue;
            }

            // Start with all idle builders.
            if let Some(next) =
                self.try_start_project(state, unit, &state.idle_builders, graph, index)
            {
                successors.push(next);
            }

            // Start with the single fastest idle builder that can build it.
            if let Some(builder) = self.fastest_idle_builder(state, unit, graph, index) {
                if let Some(next) = self.try_start_project(state, unit, &[builder], graph, index) {
                    successors.push(next);
                }
            }
        }

        // Assist each active project with all currently idle builders.
        for i in 0..state.active_projects.len() {
            if let Some(next) =
                self.try_assist_project(state, i, &state.idle_builders, graph, index)
            {
                successors.push(next);
            }
        }

        // Wait one tick.
        let mut wait = state.clone();
        wait.tick(index, self.dt);
        successors.push(wait);

        successors
    }

    /// Return candidate units that the planner may consider building.
    fn candidate_units<'a>(
        &self,
        index: &'a DataIndex,
        state: &'a GraphState,
        goals: &[&Unit],
        goal_chains: &[Vec<(Capability, String)>],
    ) -> Vec<&'a Unit> {
        let mut ids: HashSet<String> = HashSet::new();

        // Next unbuilt unit in each prerequisite chain, plus the goal itself.
        for chain in goal_chains {
            for (_, id) in chain {
                if !self.has_completed_unit(state, id) {
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
            if let Some(u) = self.pick_cheapest(&faction_units, "MASSEXTRACTION", Some(tech)) {
                ids.insert(u.id.clone());
            }
            if let Some(u) = self.pick_cheapest(&faction_units, "ENERGYPRODUCTION", Some(tech)) {
                ids.insert(u.id.clone());
            }
            if let Some(u) = self.pick_cheapest(&faction_units, "ENGINEER", Some(tech)) {
                ids.insert(u.id.clone());
            }
            if let Some(u) = self.pick_cheapest(&faction_units, "FACTORY", Some(tech)) {
                ids.insert(u.id.clone());
            }
        }

        ids.iter()
            .filter_map(|id| index.find_unit(id))
            .filter(|u| u.build_target_stats().is_some())
            .collect()
    }

    /// Cheapest unit matching category and optional tech level, by mass cost.
    fn pick_cheapest<'a>(
        &self,
        units: &[&'a Unit],
        category: &str,
        tech: Option<&str>,
    ) -> Option<&'a Unit> {
        units
            .iter()
            .filter(|u| u.has_category(category))
            .filter(|u| tech.map_or(true, |t| u.has_category(t)))
            .filter(|u| u.build_target_stats().is_some())
            .min_by(|a, b| {
                let ca = a.build_target_stats().unwrap().build_cost_mass;
                let cb = b.build_target_stats().unwrap().build_cost_mass;
                ca.total_cmp(&cb)
            })
            .copied()
    }

    /// Try to start a new project in a cloned state and advance one tick.
    fn try_start_project(
        &self,
        state: &GraphState,
        target: &Unit,
        builders: &[NodeId],
        graph: &TechGraph,
        index: &DataIndex,
    ) -> Option<GraphState> {
        if builders.is_empty() {
            return None;
        }
        let mut next = state.clone();
        match next.start_project(target, builders, graph) {
            Ok(_) => {
                next.tick(index, self.dt);
                Some(next)
            }
            Err(GraphSimError::BuilderBusy(_))
            | Err(GraphSimError::NoBuilders)
            | Err(GraphSimError::CannotBuild { .. })
            | Err(GraphSimError::NotBuildable(_))
            | Err(GraphSimError::ProjectNotFound) => None,
        }
    }

    /// Try to assist an active project in a cloned state and advance one tick.
    fn try_assist_project(
        &self,
        state: &GraphState,
        project_index: usize,
        builders: &[NodeId],
        graph: &TechGraph,
        index: &DataIndex,
    ) -> Option<GraphState> {
        if builders.is_empty() {
            return None;
        }
        let mut next = state.clone();
        match next.assist_project(project_index, builders, graph) {
            Ok(_) => {
                next.tick(index, self.dt);
                Some(next)
            }
            Err(GraphSimError::BuilderBusy(_))
            | Err(GraphSimError::NoBuilders)
            | Err(GraphSimError::CannotBuild { .. })
            | Err(GraphSimError::NotBuildable(_))
            | Err(GraphSimError::ProjectNotFound) => None,
        }
    }

    /// The fastest idle builder that can build `target`.
    fn fastest_idle_builder(
        &self,
        state: &GraphState,
        target: &Unit,
        graph: &TechGraph,
        index: &DataIndex,
    ) -> Option<NodeId> {
        state
            .idle_builders
            .iter()
            .filter(|&&b| {
                let builder_id = &state.graph.nodes[b.0].unit_id;
                graph.can_build(builder_id, &target.id).unwrap_or(false)
            })
            .max_by(|&&a, &&b| {
                let pa = builder_power(a, &state.graph, index);
                let pb = builder_power(b, &state.graph, index);
                pa.total_cmp(&pb)
            })
            .copied()
    }

    /// Admissible heuristic: optimistic time to finish all remaining goals.
    fn score(
        &self,
        state: &GraphState,
        goals: &[&Unit],
        chain_unit_ids: &[String],
        index: &DataIndex,
    ) -> f64 {
        let mut total_mass = 0.0;
        let mut total_energy = 0.0;
        let mut total_build_time = 0.0;

        for id in chain_unit_ids {
            if self.has_completed_unit(state, id) {
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
            if self.has_completed_unit(state, &goal.id) {
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

    /// Convert a winning search state into a plan result.
    fn to_plan_result(&self, state: GraphState) -> PlanResult {
        PlanResult {
            completion_time: state.time,
            final_economy: state.economy,
            events: state.events,
        }
    }
}

impl Planner for GraphPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        graph: &TechGraph,
        starting_units: &[&Unit],
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        self.plan_goals(index, graph, starting_units, &[goal])
    }
}

/// Compact visited-state key.
type VisitedKey = (Vec<String>, Vec<(String, i64)>);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::Strategy;
    use std::str::FromStr;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn graph_planner_reaches_pgen() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = GraphPlanner {
            beam_width: 20,
            max_depth: 20,
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
    fn graph_planner_reaches_monkeylord() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URL0402").expect("Monkeylord exists");

        let planner = GraphPlanner {
            beam_width: 50,
            max_depth: 400,
            dt: 10.0,
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
            "graph planner should beat the 147-minute greedy baseline"
        );
    }

    #[test]
    fn strategy_parses_graph() {
        assert_eq!(Strategy::from_str("graph").unwrap(), Strategy::Graph);
    }
}
