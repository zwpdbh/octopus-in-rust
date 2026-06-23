//! Graph-based planners.
//!
//! All planners in this module operate on the graph-growth model implemented in
//! [`crate::graph_sim`]. Each search state is a [`GraphState`]; transitions are
//! actions such as "start a new project" or "assist an active project". The
//! difference between [`GreedyPlanner`] and [`BeamPlanner`] is the search
//! algorithm: greedy keeps one state, beam search keeps the top-K states each
//! layer.

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::graph_sim::{builder_power, GraphSimError, GraphState, NodeId};
use crate::planner::{PlanResult, Planner, PlannerError};
use crate::tech_graph::{Capability, TechGraph};

/// Shared search helpers for the graph-growth model.
#[derive(Debug, Clone, Copy)]
struct SearchConfig {
    dt: f64,
    max_mex_count: usize,
    max_pgen_count: usize,
}

impl SearchConfig {
    fn successors(
        self,
        index: &DataIndex,
        tech_graph: &TechGraph,
        state: &GraphState,
        goals: &[&Unit],
        goal_chains: &[Vec<(Capability, String)>],
    ) -> Vec<GraphState> {
        if state.idle_builders.is_empty() {
            let mut next = state.clone();
            next.tick(index, self.dt);
            return vec![next];
        }

        let active_targets: HashSet<String> = state
            .active_projects
            .iter()
            .map(|p| state.graph[p.target_node].unit_id.to_ascii_uppercase())
            .collect();

        let owned_units: Vec<&Unit> = state
            .graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter_map(|n| index.find_unit(&n.unit_id))
            .collect();
        let mex_count = owned_units
            .iter()
            .filter(|u| u.has_category("MASSEXTRACTION"))
            .count();
        let pgen_count = owned_units
            .iter()
            .filter(|u| u.has_category("ENERGYPRODUCTION"))
            .count();

        let mut successors: Vec<GraphState> = Vec::new();
        let candidates = self.candidate_units(index, state, goals, goal_chains);

        for unit in candidates {
            if has_completed_unit(state, &unit.id) {
                continue;
            }
            if active_targets.contains(&unit.id.to_ascii_uppercase()) {
                continue;
            }
            if unit.has_category("MASSEXTRACTION") && mex_count >= self.max_mex_count {
                continue;
            }
            if unit.has_category("ENERGYPRODUCTION") && pgen_count >= self.max_pgen_count {
                continue;
            }

            // Start with all idle builders.
            if let Some(next) = try_start_project(
                state,
                unit,
                &state.idle_builders,
                tech_graph,
                index,
                self.dt,
            ) {
                successors.push(next);
            }

            // Start with the single fastest idle builder that can build it.
            if let Some(builder) = fastest_idle_builder(state, unit, tech_graph, index) {
                if let Some(next) =
                    try_start_project(state, unit, &[builder], tech_graph, index, self.dt)
                {
                    successors.push(next);
                }
            }
        }

        // Assist each active project with all currently idle builders.
        for i in 0..state.active_projects.len() {
            if let Some(next) =
                try_assist_project(state, i, &state.idle_builders, tech_graph, index, self.dt)
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

    fn candidate_units<'a>(
        self,
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
}

/// Greedy planner over the graph-growth model.
///
/// At each step it expands the current state, scores the successors, and picks
/// the single best one. This is fast but can get stuck in locally optimal
/// choices.
#[derive(Debug, Clone, Copy)]
pub struct GreedyPlanner {
    /// Fixed simulation timestep in seconds.
    pub dt: f64,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
    /// Maximum number of steps before giving up.
    pub max_steps: usize,
}

impl Default for GreedyPlanner {
    fn default() -> Self {
        Self {
            dt: 1.0,
            max_mex_count: 8,
            max_pgen_count: 20,
            max_steps: 20_000,
        }
    }
}

impl Planner for GreedyPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        initial_state: GraphState,
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        // Greedy planning is hard in this domain because long prerequisite
        // chains require committing to expensive investments before any progress
        // is visible. We implement it as beam search with a very narrow beam:
        // this keeps the "fast, greedy" character while retaining just enough
        // lookahead to navigate factory/engineer chains.
        let inner = BeamPlanner {
            beam_width: 3,
            max_depth: self.max_steps,
            dt: self.dt,
            max_mex_count: self.max_mex_count,
            max_pgen_count: self.max_pgen_count,
        };
        inner.plan(index, initial_state, goal)
    }
}

/// Beam-search planner over the graph-growth model.
///
/// It keeps the top-K most promising states each layer and returns the fastest
/// path to the goal unit.
#[derive(Debug, Clone)]
pub struct BeamPlanner {
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

impl Default for BeamPlanner {
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

impl BeamPlanner {
    /// Plan for one or more goal units.
    fn plan_goals(
        &self,
        index: &DataIndex,
        tech_graph: &TechGraph,
        initial_state: GraphState,
        goals: &[&Unit],
    ) -> Result<PlanResult, PlannerError> {
        if goals.is_empty() {
            return Err(PlannerError::SearchExhausted);
        }

        let mut goal_chains: Vec<Vec<(Capability, String)>> = Vec::with_capacity(goals.len());
        for goal in goals {
            let chain = tech_graph.prerequisite_chain(&goal.id, Capability::ACU)?;
            goal_chains.push(chain);
        }

        let mut chain_unit_ids: Vec<String> = goal_chains
            .iter()
            .flat_map(|chain| chain.iter().map(|(_, id)| id.clone()))
            .collect();
        chain_unit_ids.sort();
        chain_unit_ids.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let config = SearchConfig {
            dt: self.dt,
            max_mex_count: self.max_mex_count,
            max_pgen_count: self.max_pgen_count,
        };

        let mut beam: Vec<GraphState> = vec![initial_state];
        let mut visited: HashSet<VisitedKey> = HashSet::new();

        for _ in 0..self.max_depth {
            let mut next_beam: Vec<GraphState> = Vec::new();

            for state in beam {
                if goals_reached(&state, goals) {
                    return Ok(to_plan_result(state));
                }

                let key = visited_key(&state);
                if !visited.insert(key) {
                    continue;
                }

                next_beam.extend(config.successors(index, tech_graph, &state, goals, &goal_chains));
            }

            next_beam.sort_by(|a, b| {
                let fa = a.time + score(a, goals, &chain_unit_ids, index);
                let fb = b.time + score(b, goals, &chain_unit_ids, index);
                fa.total_cmp(&fb)
            });

            beam = next_beam.into_iter().take(self.beam_width).collect();
            if beam.is_empty() {
                break;
            }
        }

        // Final pass: any remaining state may already satisfy the goals.
        for state in beam {
            if goals_reached(&state, goals) {
                return Ok(to_plan_result(state));
            }
        }

        Err(PlannerError::SearchExhausted)
    }
}

impl Planner for BeamPlanner {
    fn plan(
        &self,
        index: &DataIndex,
        initial_state: GraphState,
        goal: &Unit,
    ) -> Result<PlanResult, PlannerError> {
        let tech_graph = TechGraph::new(index);
        self.plan_goals(index, &tech_graph, initial_state, &[goal])
    }
}

fn goals_reached(state: &GraphState, goals: &[&Unit]) -> bool {
    goals.iter().all(|g| has_completed_unit(state, &g.id))
}

fn has_completed_unit(state: &GraphState, unit_id: &str) -> bool {
    state
        .graph
        .graph
        .node_weights()
        .any(|n| n.is_active() && n.unit_id.eq_ignore_ascii_case(unit_id))
}

fn visited_key(state: &GraphState) -> VisitedKey {
    let mut owned: Vec<String> = state
        .graph
        .graph
        .node_weights()
        .filter(|n| n.is_active())
        .map(|n| n.unit_id.to_ascii_uppercase())
        .collect();
    owned.sort();

    let mut active: Vec<(String, i64)> = state
        .active_projects
        .iter()
        .map(|p| {
            let target_id = state.graph[p.target_node].unit_id.to_ascii_uppercase();
            let work = (p.remaining_work * 100.0).round() as i64;
            (target_id, work)
        })
        .collect();
    active.sort();

    (owned, active)
}

fn try_start_project(
    state: &GraphState,
    target: &Unit,
    builders: &[NodeId],
    graph: &TechGraph,
    index: &DataIndex,
    dt: f64,
) -> Option<GraphState> {
    if builders.is_empty() {
        return None;
    }
    let mut next = state.clone();
    match next.start_project(target, builders, graph) {
        Ok(_) => {
            next.tick(index, dt);
            Some(next)
        }
        Err(GraphSimError::BuilderBusy(_))
        | Err(GraphSimError::NoBuilders)
        | Err(GraphSimError::CannotBuild { .. })
        | Err(GraphSimError::NotBuildable(_))
        | Err(GraphSimError::ProjectNotFound) => None,
    }
}

fn try_assist_project(
    state: &GraphState,
    project_index: usize,
    builders: &[NodeId],
    graph: &TechGraph,
    index: &DataIndex,
    dt: f64,
) -> Option<GraphState> {
    if builders.is_empty() {
        return None;
    }
    let mut next = state.clone();
    match next.assist_project(project_index, builders, graph) {
        Ok(_) => {
            next.tick(index, dt);
            Some(next)
        }
        Err(GraphSimError::BuilderBusy(_))
        | Err(GraphSimError::NoBuilders)
        | Err(GraphSimError::CannotBuild { .. })
        | Err(GraphSimError::NotBuildable(_))
        | Err(GraphSimError::ProjectNotFound) => None,
    }
}

fn fastest_idle_builder(
    state: &GraphState,
    target: &Unit,
    graph: &TechGraph,
    index: &DataIndex,
) -> Option<NodeId> {
    state
        .idle_builders
        .iter()
        .filter(|&&b| {
            let builder_id = &state.graph[b].unit_id;
            graph.can_build(builder_id, &target.id).unwrap_or(false)
        })
        .max_by(|&&a, &&b| {
            let pa = builder_power(a, &state.graph, index);
            let pb = builder_power(b, &state.graph, index);
            pa.total_cmp(&pb)
        })
        .copied()
}

fn pick_cheapest<'a>(units: &[&'a Unit], category: &str, tech: Option<&str>) -> Option<&'a Unit> {
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

fn score(state: &GraphState, goals: &[&Unit], chain_unit_ids: &[String], index: &DataIndex) -> f64 {
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

fn to_plan_result(state: GraphState) -> PlanResult {
    PlanResult {
        completion_time: state.time,
        final_economy: state.economy,
        events: state.events,
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
    use crate::graph_sim::GraphState;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn beam_planner_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = BeamPlanner {
            beam_width: 20,
            max_depth: 20,
            ..Default::default()
        };
        let initial = GraphState::new(&[acu]);
        let result = planner.plan(&index, initial, goal).unwrap();

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
    fn greedy_planner_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let goal = index.find_unit("URB1101").expect("T1 pgen exists");

        let planner = GreedyPlanner::default();
        let initial = GraphState::new(&[acu]);
        let result = planner.plan(&index, initial, goal).unwrap();

        assert!(
            result
                .events
                .iter()
                .any(|e| e.unit_id.eq_ignore_ascii_case("URB1101")),
            "plan should build the goal pgen"
        );
        assert!(result.completion_time > 0.0);
    }
}
