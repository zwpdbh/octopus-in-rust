//! Shared search helpers for the graph-growth model.

use std::collections::HashSet;

use faf_units::{DataIndex, Unit};

use crate::planner::core::PlanResult;
use crate::planner::heuristic::candidate_units;
use crate::sim::{builder_power, GraphSimError, GraphState, NodeId};
use crate::tech_graph::{Capability, TechGraph};

/// Action that produced a successor state during search.
///
/// This is the planner-side analogue of a player command. Keeping it alongside
/// the successor lets a reactive planner emit the concrete command that led to
/// the best ranked state.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchAction {
    /// Build a unit with the given builders.
    Build {
        unit_id: String,
        builders: Vec<NodeId>,
    },
    /// Assist an active project with additional builders.
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Advance time without issuing a command.
    Wait,
}

/// Shared configuration and successor generation for graph-growth search.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SearchConfig {
    /// Fixed simulation timestep in seconds.
    pub dt: f64,
    /// Maximum number of mass extractors (including upgrades) to build.
    pub max_mex_count: usize,
    /// Maximum number of power generators to build.
    pub max_pgen_count: usize,
}

impl SearchConfig {
    /// Generate all successor states reachable from `state` in one search step,
    /// together with the action that produced each state.
    pub fn successors(
        self,
        index: &DataIndex,
        tech_graph: &TechGraph,
        state: &GraphState,
        goals: &[&Unit],
        goal_chains: &[Vec<(Capability, String)>],
    ) -> Vec<(GraphState, SearchAction)> {
        let idle_builders = state.idle_builders(index);
        if idle_builders.is_empty() {
            let mut next = state.clone();
            next.tick(index, self.dt);
            return vec![(next, SearchAction::Wait)];
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

        let mut successors: Vec<(GraphState, SearchAction)> = Vec::new();
        let candidates = candidate_units(index, state, goals, goal_chains);

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

            let unit_id = unit.id.clone();

            // Start with all idle builders.
            if let Some((next, builders)) =
                try_start_project(state, unit, &idle_builders, tech_graph, index, self.dt)
            {
                successors.push((
                    next,
                    SearchAction::Build {
                        unit_id: unit_id.clone(),
                        builders,
                    },
                ));
            }

            // Start with the single fastest idle builder that can build it.
            if let Some(builder) =
                fastest_idle_builder(&idle_builders, state, unit, tech_graph, index)
            {
                if let Some((next, _)) =
                    try_start_project(state, unit, &[builder], tech_graph, index, self.dt)
                {
                    successors.push((
                        next,
                        SearchAction::Build {
                            unit_id: unit_id.clone(),
                            builders: vec![builder],
                        },
                    ));
                }
            }
        }

        // Assist each active project with all currently idle builders.
        for i in 0..state.active_projects.len() {
            let project_node = state.active_projects[i].target_node;
            if let Some((next, builders)) =
                try_assist_project(state, i, &idle_builders, tech_graph, index, self.dt)
            {
                successors.push((
                    next,
                    SearchAction::Assist {
                        project_node,
                        builders,
                    },
                ));
            }
        }

        // Wait one tick.
        let mut wait = state.clone();
        wait.tick(index, self.dt);
        successors.push((wait, SearchAction::Wait));

        successors
    }
}

pub(crate) fn goals_reached(state: &GraphState, goals: &[&Unit]) -> bool {
    goals.iter().all(|g| has_completed_unit(state, &g.id))
}

pub(crate) fn has_completed_unit(state: &GraphState, unit_id: &str) -> bool {
    state
        .graph
        .graph
        .node_weights()
        .any(|n| n.is_active() && n.unit_id.eq_ignore_ascii_case(unit_id))
}

/// Compact visited-state key.
pub(crate) type VisitedKey = (Vec<String>, Vec<(String, i64)>);

pub(crate) fn visited_key(state: &GraphState) -> VisitedKey {
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

pub(crate) fn try_start_project(
    state: &GraphState,
    target: &Unit,
    builders: &[NodeId],
    graph: &TechGraph,
    index: &DataIndex,
    dt: f64,
) -> Option<(GraphState, Vec<NodeId>)> {
    if builders.is_empty() {
        return None;
    }
    let mut next = state.clone();
    let used_builders = builders.to_vec();
    match next.start_project(target, builders, graph) {
        Ok(_) => {
            next.tick(index, dt);
            Some((next, used_builders))
        }
        Err(GraphSimError::BuilderBusy(_))
        | Err(GraphSimError::NoBuilders)
        | Err(GraphSimError::CannotBuild { .. })
        | Err(GraphSimError::NotBuildable(_))
        | Err(GraphSimError::ProjectNotFound) => None,
    }
}

pub(crate) fn try_assist_project(
    state: &GraphState,
    project_index: usize,
    builders: &[NodeId],
    graph: &TechGraph,
    index: &DataIndex,
    dt: f64,
) -> Option<(GraphState, Vec<NodeId>)> {
    if builders.is_empty() {
        return None;
    }
    let mut next = state.clone();
    let used_builders = builders.to_vec();
    match next.assist_project(project_index, builders, graph) {
        Ok(_) => {
            next.tick(index, dt);
            Some((next, used_builders))
        }
        Err(GraphSimError::BuilderBusy(_))
        | Err(GraphSimError::NoBuilders)
        | Err(GraphSimError::CannotBuild { .. })
        | Err(GraphSimError::NotBuildable(_))
        | Err(GraphSimError::ProjectNotFound) => None,
    }
}

pub(crate) fn fastest_idle_builder(
    idle_builders: &[NodeId],
    state: &GraphState,
    target: &Unit,
    graph: &TechGraph,
    index: &DataIndex,
) -> Option<NodeId> {
    idle_builders
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

pub(crate) fn to_plan_result(state: GraphState, first_action: Option<SearchAction>) -> PlanResult {
    PlanResult {
        completion_time: state.time,
        final_economy: state.economy,
        events: state.events,
        first_action,
    }
}
