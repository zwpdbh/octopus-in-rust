//! Shared search helpers for the graph-growth model.
//!
//! This module is temporarily unused while the MCTS planner is being
//! implemented. The successor-generation helpers will be reused by MCTS once
//! its search loop is filled in.
#![allow(dead_code)]

use std::collections::HashSet;

use crate::planner::core::PlanResult;
use crate::planner::heuristic::candidate_units;
use crate::sim::{GraphSimError, GraphState, NodeId};
use crate::units::{UnitKind, Units};

/// Action that produced a successor state during search.
///
/// This is the planner-side analogue of a player command. Keeping it alongside
/// the successor lets a reactive planner emit the concrete command that led to
/// the best ranked state.
#[derive(Debug, Clone, PartialEq)]
pub enum SimAction {
    /// Build a unit with the given builder.
    Build { unit_id: UnitKind, builder: NodeId },
    /// Upgrade an existing unit in-place to a higher-tier blueprint.
    Upgrade {
        target_unit_id: UnitKind,
        old_node: NodeId,
        builder: NodeId,
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
    /// Maximum number of mass storage buildings to build.
    pub max_mass_storage_count: usize,
    /// Maximum number of energy storage buildings to build.
    pub max_energy_storage_count: usize,
}

impl SearchConfig {
    /// Generate all successor states reachable from `state` in one search step,
    /// together with the action that produced each state.
    ///
    /// The legal moves are: build a candidate unit, upgrade an existing unit,
    /// assist an active project, or wait.  To keep the branching factor under
    /// control, construction candidates come from [`candidate_units`], which
    /// restricts new projects to goal-relevant or highly efficient eco/builder
    /// units.
    pub fn successors(
        self,
        units: &Units,
        state: &GraphState,
        goal_id: &UnitKind,
        goal_chain: &[UnitKind],
    ) -> Vec<(GraphState, SimAction)> {
        // If nothing is idle, the only legal move is to let time advance.
        let idle_builders = state.idle_builders(units);
        if idle_builders.is_empty() {
            let mut next = state.clone();
            next.tick(units, self.dt);
            return vec![(next, SimAction::Wait)];
        }

        // Pre-compute filters used for construction candidates.
        let active_targets = state.active_target_unit_ids();
        let mex_count = state.count_active_mex();
        let pgen_count = state.count_active_pgen();
        let mass_storage_count = state.count_active_mass_storage();
        let energy_storage_count = state.count_active_energy_storage();

        // Ask the heuristic for a small menu of units worth building next.
        let candidates = candidate_units(units, state, goal_id, goal_chain);

        let mut successors: Vec<(GraphState, SimAction)> = Vec::new();

        // Apply the four successor-generation rules.
        add_build_candidates(
            &mut successors,
            state,
            units,
            self,
            &idle_builders,
            &candidates,
            &active_targets,
            mex_count,
            pgen_count,
            mass_storage_count,
            energy_storage_count,
        );
        add_upgrade_candidates(&mut successors, state, units, self, &idle_builders);
        add_assist_candidates(&mut successors, state, units, self, &idle_builders);
        add_wait_successor(&mut successors, state, units, self.dt);

        successors
    }
}

/// Rule 1: try building each candidate unit with a single builder.
///
/// A candidate is skipped if it is already built, already under construction,
/// or would exceed the configured caps (`max_mex_count` / `max_pgen_count`).
/// For each legal candidate we emit exactly one build action: start the project
/// with the fastest capable idle builder.  If the planner wants more build
/// power on the project later, it can use an `Assist` action.
fn add_build_candidates(
    successors: &mut Vec<(GraphState, SimAction)>,
    state: &GraphState,
    units: &Units,
    config: SearchConfig,
    idle_builders: &[NodeId],
    candidates: &[UnitKind],
    active_targets: &HashSet<UnitKind>,
    mex_count: usize,
    pgen_count: usize,
    mass_storage_count: usize,
    energy_storage_count: usize,
) {
    for unit_id in candidates {
        if state.has_completed_unit(unit_id) {
            continue;
        }
        if active_targets.contains(unit_id) {
            continue;
        }
        if matches!(unit_id, UnitKind::Mex(_)) && mex_count >= config.max_mex_count {
            continue;
        }
        if matches!(unit_id, UnitKind::Pgen(_)) && pgen_count >= config.max_pgen_count {
            continue;
        }
        if *unit_id == UnitKind::MassStorage && mass_storage_count >= config.max_mass_storage_count {
            continue;
        }
        if *unit_id == UnitKind::EnergyStorage
            && energy_storage_count >= config.max_energy_storage_count
        {
            continue;
        }

        // Start the project with the single fastest capable idle builder.
        // Additional builders can be added later with `Assist`.
        if let Some(builder) = fastest_idle_builder(idle_builders, state, unit_id, units) {
            if let Some(next) = try_start_project(state, unit_id, builder, units, config.dt) {
                successors.push((
                    next,
                    SimAction::Build {
                        unit_id: unit_id.clone(),
                        builder,
                    },
                ));
            }
        }
    }
}

/// Rule 2: try upgrading finished units with a single builder.
///
/// For every finished active unit that has a registered upgrade target, emit
/// one action: start the upgrade with the fastest capable idle builder.
/// Additional builders can be added later with `Assist`.
fn add_upgrade_candidates(
    successors: &mut Vec<(GraphState, SimAction)>,
    state: &GraphState,
    units: &Units,
    config: SearchConfig,
    idle_builders: &[NodeId],
) {
    for &old_node in &state.active_units() {
        let old_kind = state.graph[old_node].unit_id.clone();
        let Some(target) = units.upgrade_target(&old_kind) else {
            continue;
        };

        // Start the upgrade with the single fastest capable idle builder.
        if let Some(builder) = fastest_idle_builder(idle_builders, state, &target, units) {
            if let Some(next) =
                try_upgrade_project(state, &target, old_node, builder, units, config.dt)
            {
                successors.push((
                    next,
                    SimAction::Upgrade {
                        target_unit_id: target.clone(),
                        old_node,
                        builder,
                    },
                ));
            }
        }
    }
}

/// Rule 3: try assisting each active project with all idle builders.
fn add_assist_candidates(
    successors: &mut Vec<(GraphState, SimAction)>,
    state: &GraphState,
    units: &Units,
    config: SearchConfig,
    idle_builders: &[NodeId],
) {
    let active_projects: Vec<NodeId> = state
        .graph
        .graph
        .node_weights()
        .filter(|n| {
            matches!(
                n.state,
                crate::sim::UnitNodeState::Constructing { .. }
                    | crate::sim::UnitNodeState::Upgrading { .. }
            )
        })
        .map(|n| n.id)
        .collect();

    for project_node in active_projects {
        if let Some((next, builders)) =
            try_assist_project(state, project_node, idle_builders, units, config.dt)
        {
            successors.push((
                next,
                SimAction::Assist {
                    project_node,
                    builders,
                },
            ));
        }
    }
}

/// Rule 4: always allow advancing time without issuing a command.
fn add_wait_successor(
    successors: &mut Vec<(GraphState, SimAction)>,
    state: &GraphState,
    units: &Units,
    dt: f64,
) {
    let mut wait = state.clone();
    wait.tick(units, dt);
    successors.push((wait, SimAction::Wait));
}

/// Compact visited-state key.
pub(crate) type VisitedKey = (Vec<UnitKind>, Vec<(UnitKind, i64)>);

pub(crate) fn visited_key(state: &GraphState) -> VisitedKey {
    let mut owned: Vec<UnitKind> = state
        .graph
        .graph
        .node_weights()
        .filter(|n| n.is_active())
        .map(|n| n.unit_id.clone())
        .collect();
    owned.sort();

    let mut active: Vec<(UnitKind, i64)> = state
        .graph
        .graph
        .node_weights()
        .filter(|n| {
            matches!(
                n.state,
                crate::sim::UnitNodeState::Constructing { .. }
                    | crate::sim::UnitNodeState::Upgrading { .. }
            )
        })
        .map(|n| {
            let target = n.unit_id.clone();
            let work = (n.remaining_work().unwrap_or(0.0) * 100.0).round() as i64;
            (target, work)
        })
        .collect();
    active.sort();

    (owned, active)
}

/// Try starting a project for `target` with a single `builder`.
///
/// On success, returns the state after the project is started and one tick has
/// elapsed.  On failure (busy builder, cannot build, etc.) returns `None`.
pub(crate) fn try_start_project(
    state: &GraphState,
    target: &UnitKind,
    builder: NodeId,
    units: &Units,
    dt: f64,
) -> Option<GraphState> {
    let mut next = state.clone();
    match next.start_project(target, &[builder], units) {
        Ok(_) => {
            next.tick(units, dt);
            Some(next)
        }
        Err(GraphSimError::BuilderBusy(_))
        | Err(GraphSimError::NoBuilders)
        | Err(GraphSimError::CannotBuild { .. })
        | Err(GraphSimError::NotBuildable(_))
        | Err(GraphSimError::ProjectNotFound) => None,
    }
}

/// Try upgrading `old_node` to `target` with a single `builder`.
///
/// On success, returns the state after the upgrade is started and one tick has
/// elapsed.  On failure returns `None`.
pub(crate) fn try_upgrade_project(
    state: &GraphState,
    target: &UnitKind,
    old_node: NodeId,
    builder: NodeId,
    units: &Units,
    dt: f64,
) -> Option<GraphState> {
    let mut next = state.clone();
    match next.start_upgrade_project(target, old_node, &[builder], units) {
        Ok(_) => {
            next.tick(units, dt);
            Some(next)
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
    project_node: NodeId,
    builders: &[NodeId],
    units: &Units,
    dt: f64,
) -> Option<(GraphState, Vec<NodeId>)> {
    if builders.is_empty() {
        return None;
    }
    let mut next = state.clone();
    let used_builders = builders.to_vec();
    match next.assist_project(project_node, builders, units) {
        Ok(_) => {
            next.tick(units, dt);
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
    target: &UnitKind,
    units: &Units,
) -> Option<NodeId> {
    idle_builders
        .iter()
        .filter(|&&b| {
            let builder_kind = &state.graph[b].unit_id;
            units.can_build(builder_kind, target)
        })
        .max_by(|&&a, &&b| {
            let rate_a = units
                .def(&state.graph[a].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            let rate_b = units
                .def(&state.graph[b].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            rate_a.total_cmp(&rate_b)
        })
        .copied()
}

pub(crate) fn to_plan_result(state: GraphState, first_action: Option<SimAction>) -> PlanResult {
    PlanResult {
        completion_time: state.time,
        final_economy: state.economy,
        events: state.events,
        first_action,
    }
}
