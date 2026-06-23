//! Beam-search planner over the graph-growth model.
//!
//! It keeps the top-K most promising states each layer and returns the fastest
//! path to the goal unit.

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::planner::core::{PlanResult, Planner, PlannerError};
use crate::planner::search::{
    goals_reached, score, to_plan_result, visited_key, SearchConfig, VisitedKey,
};
use crate::sim::GraphState;
use crate::tech_graph::{Capability, TechGraph};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::GraphState;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
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
}
