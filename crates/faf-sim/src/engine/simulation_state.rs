//! Legacy mutable simulation state for the graph model.
//!
//! `SimulationState` is preserved temporarily while planners and trainers are
//! migrated to [`UnitGraph`](crate::engine::unit_graph::UnitGraph) and
//! [`EcoEngine`](crate::engine::engine::EcoEngine).

use std::collections::HashSet;

use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::economy::{apply_tick_graph, compute_drain, EconomyState, RequestedBuildPower};
use crate::engine::adjacency::{production_multiplier, AdjacencyKind, AdjacencyTracker};
use crate::engine::unit_graph::{
    builder_power, BuildEdge, BuildEvent, BuildGraph, GraphSimError, NodeId, UnitNode,
    UnitNodeState,
};
use crate::planner::core::Goal;
use crate::quantities::{EnergyRate, MassRate};
use crate::units::{TechLevel, UnitDef, UnitKind, Units};

/// An active abstract-goal project.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalProject {
    /// The abstract target being built.
    pub goal: Goal,
    /// Remaining work in blueprint build-time units.
    pub remaining_work: f64,
    /// Builders capable of building the goal.
    pub started_by: Vec<NodeId>,
    /// Additional builders assisting construction.
    pub assisted_by: Vec<NodeId>,
    /// True once the remaining work reaches zero.
    pub completed: bool,
}

/// Mutable simulation state for the graph model.
#[derive(Debug, Clone)]
pub struct SimulationState {
    /// Current simulation time in seconds.
    pub time: f64,
    /// The build graph.
    pub graph: BuildGraph,
    /// Current economy state.
    pub economy: EconomyState,
    /// Completed build events in chronological order.
    pub events: Vec<BuildEvent>,
    /// Adjacency bonuses for mass and energy production.
    pub adjacency: AdjacencyTracker,
    /// The active abstract-goal project, if one has been started.
    pub goal_project: Option<GoalProject>,
}

/// True if the node represents an active builder unit.
fn is_builder_node(node_id: NodeId, graph: &BuildGraph, units: &Units) -> bool {
    let node = &graph[node_id];
    if !node.is_active() {
        return false;
    }
    units
        .def(&node.unit_id)
        .is_some_and(|d| d.build_rate() > 0.0)
}

impl SimulationState {
    /// Create a new simulation state from the given starting unit kinds.
    ///
    /// All starting units are treated as already completed at time 0. Any
    /// builders among them are added to `idle_builders`.
    pub fn new(units: &Units, starting_units: &[UnitKind]) -> Self {
        let mut graph = BuildGraph::default();

        for (i, kind) in starting_units.iter().enumerate() {
            let node_id = NodeId::new(i);
            graph.graph.add_node(UnitNode {
                id: node_id,
                unit_id: kind.clone(),
                state: UnitNodeState::Constructed {
                    start_time: 0.0,
                    finish_time: 0.0,
                },
            });
        }

        let mut adjacency = AdjacencyTracker::new();
        for node in graph.graph.node_weights() {
            if matches!(node.unit_id, UnitKind::CapT2Mex | UnitKind::CapT3Mex) {
                adjacency.set(
                    AdjacencyKind::Mass,
                    node.id,
                    crate::engine::adjacency::MAX_ADJACENCY,
                );
            }
        }

        let mut state = Self {
            time: 0.0,
            graph,
            economy: EconomyState::default(),
            events: Vec::new(),
            adjacency,
            goal_project: None,
        };
        state.rebuild_economy(units);
        state
    }

    /// Return the builder nodes that are currently idle and available for new
    /// work.
    ///
    /// This is derived from `graph`: a builder is busy if it has an outgoing
    /// edge to a node that is still under construction or upgrade.
    pub fn idle_builders(&self, units: &Units) -> Vec<NodeId> {
        let busy = self.busy_builders();

        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter(|n| is_builder_node(n.id, &self.graph, units))
            .map(|n| n.id)
            .filter(|id| !busy.contains(id))
            .collect()
    }

    /// Return the ids of all finished units in the graph.
    pub fn finished_units(&self) -> Vec<NodeId> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_finished())
            .map(|n| n.id)
            .collect()
    }

    /// Return the ids of all active units: finished units that are not currently
    /// upgrading.
    pub fn active_units(&self) -> Vec<NodeId> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .map(|n| n.id)
            .collect()
    }

    /// True if the unit represented by `node_id` has finished construction or
    /// upgrade.
    pub fn is_completed(&self, node_id: NodeId) -> bool {
        self.graph
            .graph
            .node_weight(node_id.0)
            .is_some_and(|n| n.is_finished())
    }

    /// True if the unit represented by `node_id` is active (finished and not
    /// currently upgrading).
    pub fn is_active(&self, node_id: NodeId) -> bool {
        self.graph
            .graph
            .node_weight(node_id.0)
            .is_some_and(|n| n.is_active())
    }

    /// True if an active unit with the given kind has been completed.
    pub fn has_completed_unit(&self, unit_id: &UnitKind) -> bool {
        self.graph
            .graph
            .node_weights()
            .any(|n| n.is_active() && n.unit_id == *unit_id)
    }

    /// True if the given abstract goal has been completed.
    pub fn goal_reached(&self, goal: &Goal) -> bool {
        self.goal_project
            .as_ref()
            .is_some_and(|p| p.completed && p.goal == *goal)
    }

    /// True if an abstract goal project is currently under construction.
    pub fn goal_project_active(&self) -> bool {
        self.goal_project.as_ref().is_some_and(|p| !p.completed)
    }

    /// Return the kinds of all units currently under construction.
    pub fn active_target_unit_ids(&self) -> HashSet<UnitKind> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| {
                matches!(
                    n.state,
                    UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
                )
            })
            .map(|n| n.unit_id.clone())
            .collect()
    }

    /// Count how many active units have the given kind.
    pub fn count_active_by_kind(&self, kind: &UnitKind) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active() && n.unit_id == *kind)
            .count()
    }

    /// Count how many active mass extractors (any tech level, including capped)
    /// are in the graph.
    pub fn count_active_mex(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| {
                n.is_active()
                    && (matches!(n.unit_id, UnitKind::Mex(_))
                        || matches!(n.unit_id, UnitKind::CapT2Mex | UnitKind::CapT3Mex))
            })
            .count()
    }

    /// Count how many active power generators (any tech level) are in the graph.
    pub fn count_active_pgen(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active() && matches!(n.unit_id, UnitKind::Pgen(_)))
            .count()
    }

    /// Count how many active energy storage buildings are in the graph.
    pub fn count_active_energy_storage(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active() && n.unit_id == UnitKind::EnergyStorage)
            .count()
    }

    /// Return the definitions for every active unit in the graph.
    pub fn active_unit_blueprints<'a>(&'a self, units: &'a Units) -> Vec<&'a UnitDef> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter_map(|n| units.def(&n.unit_id))
            .collect()
    }

    /// Total build power of all active builders, including idle builders and
    /// builders currently assigned to active projects.
    pub fn total_active_build_power(&self, units: &Units) -> f64 {
        self.active_units()
            .iter()
            .map(|&b| builder_power(b, &self.graph, units))
            .sum()
    }

    /// Re-derive the economy from all active units, applying adjacency bonuses
    /// to producers.
    pub fn rebuild_economy(&mut self, units: &Units) {
        let active_nodes: Vec<NodeId> = self.active_units();

        let mut net_mass = MassRate::zero();
        let mut net_energy = EnergyRate::zero();
        let mut mass_storage_cap = crate::quantities::Mass::zero();
        let mut energy_storage_cap = crate::quantities::Energy::zero();

        for node_id in active_nodes {
            let kind = &self.graph[node_id].unit_id;
            let Some(def) = units.def(kind) else {
                continue;
            };

            let mut mass_income = def.mass_income();
            let mut energy_income = def.energy_income();

            // Apply the unified FAF adjacency bonus: +12.5% per adjacent storage,
            // capped at 4 storages (+50% max).
            if AdjacencyKind::Mass.is_producer(kind) {
                let caps = self.adjacency.count(AdjacencyKind::Mass, node_id);
                mass_income *= production_multiplier(caps);
            }
            if AdjacencyKind::Energy.is_producer(kind) {
                let caps = self.adjacency.count(AdjacencyKind::Energy, node_id);
                energy_income *= production_multiplier(caps);
            }

            net_mass = net_mass + MassRate::from_raw(mass_income);
            net_energy =
                net_energy + EnergyRate::from_raw(energy_income - def.maintenance_energy());
            mass_storage_cap =
                mass_storage_cap + crate::quantities::Mass::from_raw(def.mass_storage());
            energy_storage_cap =
                energy_storage_cap + crate::quantities::Energy::from_raw(def.energy_storage());
        }

        self.economy.net_mass_income = net_mass;
        self.economy.net_energy_income = net_energy;
        self.economy.mass_storage = mass_storage_cap;
        self.economy.energy_storage = energy_storage_cap;
        self.economy.mass_storage_cap = mass_storage_cap;
        self.economy.energy_storage_cap = energy_storage_cap;
    }

    /// Assign a newly completed energy storage building to an active power
    /// generator using the unified adjacency tracker.
    fn assign_energy_storage_cap(&mut self) {
        let active: Vec<NodeId> = self
            .graph
            .graph
            .node_weights()
            .filter(|n| n.is_active() && AdjacencyKind::Energy.is_producer(&n.unit_id))
            .map(|n| n.id)
            .collect();

        self.adjacency.assign_to_least_capped(
            AdjacencyKind::Energy,
            active.into_iter(),
            |node_id| AdjacencyKind::Energy.is_producer(&self.graph[node_id].unit_id),
        );
    }

    /// Return the set of builders currently assigned to an active project.
    fn busy_builders(&self) -> HashSet<NodeId> {
        let mut busy: HashSet<NodeId> = self
            .graph
            .graph
            .node_weights()
            .filter(|n| {
                matches!(
                    n.state,
                    UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
                )
            })
            .flat_map(|n| self.graph.graph.edges_directed(n.id.0, Direction::Incoming))
            .map(|edge| NodeId::new(edge.source().index()))
            .collect();

        if let Some(ref gp) = self.goal_project {
            if !gp.completed {
                busy.extend(gp.started_by.iter());
                busy.extend(gp.assisted_by.iter());
            }
        }
        busy
    }

    /// Validate that every builder in `builders` is idle and is a real builder.
    fn validate_builders(
        &self,
        builders: &[NodeId],
        target: &UnitKind,
        units: &Units,
    ) -> Result<(), GraphSimError> {
        if builders.is_empty() {
            return Err(GraphSimError::NoBuilders);
        }

        let busy = self.busy_builders();

        for &builder in builders {
            if busy.contains(&builder) {
                return Err(GraphSimError::BuilderBusy(builder));
            }
            if !is_builder_node(builder, &self.graph, units) {
                return Err(GraphSimError::CannotBuild {
                    builder: self.graph[builder].unit_id.clone(),
                    target: target.clone(),
                });
            }
        }
        Ok(())
    }

    /// Start a new project to build `target` using the given idle `builders`.
    ///
    /// At least one builder must be capable of building `target`; the remaining
    /// builders assist. Returns the node id of the new target unit.
    pub fn start_project(
        &mut self,
        target: &UnitKind,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, target, units)?;

        let cost = units
            .build_cost(target)
            .ok_or_else(|| GraphSimError::NotBuildable(target.clone()))?;

        let (started_by, assisted_by) = self.split_builders(builders, target, units);
        if started_by.is_empty() {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph[*b].unit_id.clone())
                    .unwrap_or_else(|| target.clone()),
                target: target.clone(),
            });
        }

        let node_id = NodeId::new(self.graph.graph.node_count());
        self.graph.graph.add_node(UnitNode {
            id: node_id,
            unit_id: target.clone(),
            state: UnitNodeState::Constructing {
                start: self.time,
                remaining_work: cost.build_time,
                started_by: started_by.clone(),
                assisted_by: assisted_by.clone(),
            },
        });

        for &builder in builders {
            self.graph.graph.add_edge(
                builder.0,
                node_id.0,
                BuildEdge {
                    start_time: self.time,
                    finish_time: f64::NAN,
                },
            );
        }

        Ok(node_id)
    }

    /// Start a new abstract-goal project using the given idle `builders`.
    ///
    /// At least one builder must be a T3 engineer. The remaining builders assist.
    pub fn start_goal_project(
        &mut self,
        goal: Goal,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<(), GraphSimError> {
        if self.goal_project_active() {
            return Err(GraphSimError::GoalProjectActive);
        }

        self.validate_builders(builders, &UnitKind::Commander, units)?;

        let mut started_by = Vec::new();
        let mut assisted_by = Vec::new();
        for &builder in builders {
            if matches!(
                self.graph[builder].unit_id,
                UnitKind::Engineer(TechLevel::T3)
            ) {
                started_by.push(builder);
            } else {
                assisted_by.push(builder);
            }
        }

        if started_by.is_empty() {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph[*b].unit_id.clone())
                    .unwrap_or_else(|| UnitKind::Commander),
                target: UnitKind::Commander,
            });
        }

        self.goal_project = Some(GoalProject {
            goal,
            remaining_work: goal.cost().build_time,
            started_by,
            assisted_by,
            completed: false,
        });

        Ok(())
    }

    /// Split builders into those capable of building the target and assistants.
    fn split_builders(
        &self,
        builders: &[NodeId],
        target: &UnitKind,
        units: &Units,
    ) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut started_by = Vec::new();
        let mut assisted_by = Vec::new();
        for &builder in builders {
            let builder_unit_id = &self.graph[builder].unit_id;
            if units.can_build(builder_unit_id, target) {
                started_by.push(builder);
            } else {
                assisted_by.push(builder);
            }
        }
        (started_by, assisted_by)
    }

    /// Start an upgrade of `old_node` to `target` using the given idle builders.
    ///
    /// A upgrade affects two nodes:
    /// - `old_node` is marked `Replaced { into: new_node }` so it no longer
    ///   contributes to the economy or acts as a builder.
    /// - A new node is added to the graph for the upgraded unit. It starts in
    ///   the `Upgrading { from_unit_id: old_kind }` state and finishes
    ///   as `Upgraded { from_unit_id: old_kind }`.
    pub fn start_upgrade_project(
        &mut self,
        target: &UnitKind,
        old_node: NodeId,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, target, units)?;

        let old_unit_id = self.graph[old_node].unit_id.clone();
        let recipe = units
            .upgrade_recipes(&old_unit_id)
            .iter()
            .find(|r| r.to == *target)
            .ok_or_else(|| GraphSimError::NotBuildable(target.clone()))?;
        let cost = recipe.cost;

        let (started_by, assisted_by) = {
            let mut started_by = Vec::new();
            let mut assisted_by = Vec::new();
            for &builder in builders {
                let builder_kind = &self.graph[builder].unit_id;
                if recipe.builder_options.contains(builder_kind) {
                    started_by.push(builder);
                } else {
                    assisted_by.push(builder);
                }
            }
            (started_by, assisted_by)
        };
        if started_by.is_empty() {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph[*b].unit_id.clone())
                    .unwrap_or_else(|| target.clone()),
                target: target.clone(),
            });
        }

        // Capture the old node's construction timing before retiring it.
        let (old_start, old_finish) = match &self.graph[old_node].state {
            UnitNodeState::Constructed {
                start_time,
                finish_time,
            } => (*start_time, *finish_time),
            UnitNodeState::Upgraded {
                start_time,
                finish_time,
                ..
            } => (*start_time, *finish_time),
            _ => (self.time, self.time),
        };

        let new_node = NodeId::new(self.graph.graph.node_count());
        self.graph.graph.add_node(UnitNode {
            id: new_node,
            unit_id: target.clone(),
            state: UnitNodeState::Upgrading {
                start: self.time,
                remaining_work: cost.build_time,
                from_unit_id: old_unit_id.clone(),
                started_by: started_by.clone(),
                assisted_by: assisted_by.clone(),
            },
        });

        self.graph[old_node].state = UnitNodeState::Replaced {
            start_time: old_start,
            finish_time: old_finish,
            into: new_node,
        };

        for &builder in builders {
            self.graph.graph.add_edge(
                builder.0,
                new_node.0,
                BuildEdge {
                    start_time: self.time,
                    finish_time: f64::NAN,
                },
            );
        }

        Ok(new_node)
    }

    /// Assign additional idle `builders` to an already active project.
    ///
    /// Assisting builders do not need to be capable of building the target;
    /// they only need to be real builders.
    pub fn assist_project(
        &mut self,
        target_node: NodeId,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<(), GraphSimError> {
        // Assist has no meaningful target for error messages; use a placeholder.
        self.validate_builders(builders, &UnitKind::Commander, units)?;

        if !matches!(
            self.graph[target_node].state,
            UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
        ) {
            return Err(GraphSimError::ProjectNotFound);
        }

        for &builder in builders {
            self.graph.graph.add_edge(
                builder.0,
                target_node.0,
                BuildEdge {
                    start_time: self.time,
                    finish_time: f64::NAN,
                },
            );
        }

        Ok(())
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Returns the node ids of any units that completed during this tick.
    pub fn tick(&mut self, units: &Units, dt: f64) -> Vec<NodeId> {
        if dt <= 0.0 {
            return Vec::new();
        }

        let active_projects = self.collect_active_projects();
        if active_projects.is_empty() && !self.goal_project_active() {
            self.apply_idle_income(dt);
            self.time += dt;
            return Vec::new();
        }

        // 1. Compute resource drain from all active work.
        let (project_powers, project_mass_drain, project_energy_drain) =
            self.compute_project_drain(&active_projects, units);
        let (goal_power, goal_mass_drain, goal_energy_drain) = self.compute_goal_drain(units);

        let total_mass_drain = project_mass_drain + goal_mass_drain;
        let total_energy_drain = project_energy_drain + goal_energy_drain;

        // 2. Apply economy tick: storage changes and effective progress factor.
        let tick_result = apply_tick_graph(total_mass_drain, total_energy_drain, &self.economy, dt);

        // 3. Apply progress to projects and the abstract goal.
        let completed_nodes = self.apply_project_progress(
            &active_projects,
            &project_powers,
            tick_result.effective_factor,
            dt,
        );
        self.apply_goal_progress(goal_power, tick_result.effective_factor, dt);

        // 4. Commit storage and time advances.
        self.economy.mass_storage = tick_result.new_mass_storage;
        self.economy.energy_storage = tick_result.new_energy_storage;
        self.time += dt;

        // 5. Handle side effects of completed nodes.
        self.process_completed_nodes(&completed_nodes, units);

        completed_nodes
    }

    /// Collect all nodes currently under construction or upgrade.
    fn collect_active_projects(&self) -> Vec<NodeId> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| {
                matches!(
                    n.state,
                    UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
                )
            })
            .map(|n| n.id)
            .collect()
    }

    /// Compute build power and resource drain for each active project.
    ///
    /// Returns a parallel vector of project powers and the total mass/energy drain.
    fn compute_project_drain(
        &self,
        active_projects: &[NodeId],
        units: &Units,
    ) -> (Vec<f64>, f64, f64) {
        let mut project_powers: Vec<f64> = Vec::with_capacity(active_projects.len());
        let mut total_mass_drain = 0.0;
        let mut total_energy_drain = 0.0;

        for &target_node in active_projects {
            let target_id = &self.graph[target_node].unit_id;
            let Some(cost) = units.build_cost(target_id) else {
                project_powers.push(0.0);
                continue;
            };

            let power: f64 = self
                .graph
                .graph
                .edges_directed(target_node.0, Direction::Incoming)
                .map(|edge| builder_power(NodeId::new(edge.source().index()), &self.graph, units))
                .sum();
            project_powers.push(power);

            let Some(drain) = compute_drain(&cost.to_target_stats(), RequestedBuildPower(power))
            else {
                continue;
            };
            total_mass_drain += drain.mass_per_second;
            total_energy_drain += drain.energy_per_second;
        }

        (project_powers, total_mass_drain, total_energy_drain)
    }

    /// Compute build power and resource drain for the active abstract goal project, if any.
    fn compute_goal_drain(&self, units: &Units) -> (f64, f64, f64) {
        let mut goal_power = 0.0;
        let mut mass_drain = 0.0;
        let mut energy_drain = 0.0;

        let Some(ref gp) = self.goal_project else {
            return (goal_power, mass_drain, energy_drain);
        };
        if gp.completed {
            return (goal_power, mass_drain, energy_drain);
        }

        let power: f64 = gp
            .started_by
            .iter()
            .chain(gp.assisted_by.iter())
            .map(|&id| builder_power(id, &self.graph, units))
            .sum();
        goal_power = power;

        if let Some(drain) = compute_drain(
            &gp.goal.cost().to_target_stats(),
            RequestedBuildPower(power),
        ) {
            mass_drain = drain.mass_per_second;
            energy_drain = drain.energy_per_second;
        } else {
            goal_power = 0.0;
        }

        (goal_power, mass_drain, energy_drain)
    }

    /// Apply progress to all active projects and return any that finished.
    fn apply_project_progress(
        &mut self,
        active_projects: &[NodeId],
        project_powers: &[f64],
        effective_factor: f64,
        dt: f64,
    ) -> Vec<NodeId> {
        let mut completed_nodes = Vec::new();

        for (i, &target_node) in active_projects.iter().enumerate() {
            let power = project_powers[i];
            if power <= 0.0 {
                continue;
            }
            let progress = effective_factor * power * dt;

            let (finished, work_before) = match &mut self.graph[target_node].state {
                UnitNodeState::Constructing { remaining_work, .. }
                | UnitNodeState::Upgrading { remaining_work, .. } => {
                    let before = *remaining_work;
                    *remaining_work -= progress;
                    (*remaining_work <= 0.0, before)
                }
                _ => continue,
            };

            if finished {
                let fraction = if progress > 0.0 {
                    (work_before / progress).min(1.0)
                } else {
                    1.0
                };
                let finish_time = self.time + fraction * dt;
                self.finish_project_node(target_node, finish_time);
                completed_nodes.push(target_node);
            }
        }

        completed_nodes
    }

    /// Transition a single project node from in-progress to finished and record builder-edge finish times.
    fn finish_project_node(&mut self, target_node: NodeId, finish_time: f64) {
        let node = &mut self.graph[target_node];
        let (start_time, from_unit_id) = match &node.state {
            UnitNodeState::Constructing { start, .. } => (*start, None),
            UnitNodeState::Upgrading {
                start,
                from_unit_id,
                ..
            } => (*start, Some(from_unit_id.clone())),
            _ => (self.time, None),
        };
        node.state = match from_unit_id {
            Some(from_unit_id) => UnitNodeState::Upgraded {
                start_time,
                finish_time,
                from_unit_id,
            },
            None => UnitNodeState::Constructed {
                start_time,
                finish_time,
            },
        };

        // Record the finish time on every builder assignment edge.
        let edge_ids: Vec<_> = self
            .graph
            .graph
            .edges_directed(target_node.0, Direction::Incoming)
            .map(|edge| edge.id())
            .collect();
        for edge_id in edge_ids {
            if let Some(weight) = self.graph.graph.edge_weight_mut(edge_id) {
                weight.finish_time = finish_time;
            }
        }
    }

    /// Apply progress to the abstract goal project, if it is active and affordable.
    fn apply_goal_progress(&mut self, goal_power: f64, effective_factor: f64, dt: f64) {
        if goal_power <= 0.0 {
            return;
        }
        let Some(ref mut gp) = self.goal_project else {
            return;
        };
        if gp.completed {
            return;
        }

        let progress = effective_factor * goal_power * dt;
        gp.remaining_work -= progress;
        if gp.remaining_work <= 0.0 {
            gp.completed = true;
        }
    }

    /// Handle side effects of completed nodes: adjacency, build events, and economy rebuild.
    fn process_completed_nodes(&mut self, completed_nodes: &[NodeId], units: &Units) {
        self.apply_completion_adjacency(completed_nodes);
        self.emit_build_events(completed_nodes, units);

        if !completed_nodes.is_empty() {
            self.rebuild_economy(units);
        }
    }

    /// Apply adjacency bonuses for newly completed storage and capped mexes.
    fn apply_completion_adjacency(&mut self, completed_nodes: &[NodeId]) {
        for &target_node in completed_nodes {
            let kind = self.graph[target_node].unit_id.clone();
            if matches!(kind, UnitKind::EnergyStorage) {
                self.assign_energy_storage_cap();
            }
            if matches!(kind, UnitKind::CapT2Mex | UnitKind::CapT3Mex) {
                self.adjacency.set(
                    AdjacencyKind::Mass,
                    target_node,
                    crate::engine::adjacency::MAX_ADJACENCY,
                );
            }
        }
    }

    /// Emit build events for all nodes that completed this tick.
    fn emit_build_events(&mut self, completed_nodes: &[NodeId], units: &Units) {
        for &target_node in completed_nodes {
            let node = &self.graph[target_node];
            let unit_id = node.unit_id.clone();
            let finish_time = match &node.state {
                UnitNodeState::Constructed { finish_time, .. } => *finish_time,
                UnitNodeState::Upgraded { finish_time, .. } => *finish_time,
                _ => self.time,
            };

            self.events.push(BuildEvent {
                time: finish_time,
                unit_id: unit_id.clone(),
                unit_name: units.display_name(&unit_id),
                node_id: target_node,
            });
        }
    }

    /// Collect income for one tick with no active projects.
    fn apply_idle_income(&mut self, dt: f64) {
        let dt = crate::quantities::Time::from_raw(dt);
        self.economy.mass_storage = (self.economy.mass_storage + self.economy.net_mass_income * dt)
            .min(self.economy.mass_storage_cap)
            .max(crate::quantities::Mass::zero());
        self.economy.energy_storage = (self.economy.energy_storage
            + self.economy.net_energy_income * dt)
            .min(self.economy.energy_storage_cap)
            .max(crate::quantities::Energy::zero());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitId};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn acu_builds_t1_pgen() {
        let units = load_units();
        let acu = units.def(&UnitKind::Commander).expect("ACU exists");
        let pgen = units
            .def(&UnitKind::Pgen(TechLevel::T1))
            .expect("T1 pgen exists");

        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        let acu_node = NodeId::new(0);
        state
            .start_project(&UnitKind::Pgen(TechLevel::T1), &[acu_node], &units)
            .expect("ACU can build T1 pgen");

        let acu_rate = acu.build_rate();
        let expected_ticks = (pgen.cost.build_time / acu_rate).ceil();
        let mut completed = Vec::new();
        for _ in 0..(expected_ticks as usize + 5) {
            completed.extend(state.tick(&units, 1.0));
            if !completed.is_empty() {
                break;
            }
        }

        assert_eq!(completed.len(), 1);
        assert_eq!(
            state.graph[completed[0]].unit_id,
            UnitKind::Pgen(TechLevel::T1)
        );
        assert!(state.time > 0.0);
        assert!(state.idle_builders(&units).contains(&acu_node));
    }

    #[test]
    fn capped_t2_mex_boosts_income() {
        let units = load_units();
        let mut state = SimulationState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Engineer(TechLevel::T1),
                UnitKind::Mex(TechLevel::T2),
            ],
        );
        let eng_node = NodeId::new(1);
        let mex_node = NodeId::new(2);
        let base_mass = state.economy.net_mass_income;

        let cap_node = state
            .start_upgrade_project(&UnitKind::CapT2Mex, mex_node, &[eng_node], &units)
            .expect("engineer caps t2 mex");
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(cap_node) {
                break;
            }
        }
        assert!(state.is_completed(cap_node));

        let t2_mex_def = units.def(&UnitKind::Mex(TechLevel::T2)).unwrap();
        // The T2 mex is retired and replaced by a CapT2Mex, so the delta is the
        // +50% adjacency bonus over the base T2 mex income.
        let expected_boost = t2_mex_def.mass_income() * 0.5;
        assert!(
            (state.economy.net_mass_income
                - base_mass
                - crate::quantities::MassRate::from_raw(expected_boost))
            .abs()
                < 1e-6,
            "expected mass income boost of {}, got {}",
            expected_boost,
            state.economy.net_mass_income.value() - base_mass.value()
        );
    }

    #[test]
    fn energy_storage_boosts_pgen_income() {
        let units = load_units();
        let mut state = SimulationState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Engineer(TechLevel::T1),
                UnitKind::Pgen(TechLevel::T1),
            ],
        );
        let eng_node = NodeId::new(1);
        let pgen_node = NodeId::new(2);
        let base_energy = state.economy.net_energy_income;

        let storage_node = state
            .start_project(&UnitKind::EnergyStorage, &[eng_node], &units)
            .expect("engineer builds energy storage");
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(storage_node) {
                break;
            }
        }
        assert!(state.is_completed(storage_node));

        let pgen_def = units.def(&UnitKind::Pgen(TechLevel::T1)).unwrap();
        let expected_boost = 0.125 * pgen_def.energy_income();
        assert!(
            (state.economy.net_energy_income
                - base_energy
                - crate::quantities::EnergyRate::from_raw(expected_boost))
            .abs()
                < 1e-6,
            "expected energy income boost of {}, got {}",
            expected_boost,
            state.economy.net_energy_income.value() - base_energy.value()
        );
        assert_eq!(state.adjacency.count(AdjacencyKind::Energy, pgen_node), 1);
    }

    #[test]
    fn build_edge_records_interval() {
        let units = load_units();

        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        let acu_node = NodeId::new(0);
        let pgen_node = state
            .start_project(&UnitKind::Pgen(TechLevel::T1), &[acu_node], &units)
            .expect("ACU can build T1 pgen");

        let edge_idx = state
            .graph
            .graph
            .find_edge(acu_node.0, pgen_node.0)
            .expect("edge exists");
        let edge = state
            .graph
            .graph
            .edge_weight(edge_idx)
            .expect("edge has weight");
        assert_eq!(edge.start_time, 0.0);
        assert!(edge.finish_time.is_nan(), "active edge has no finish time");

        // Tick until the pgen completes.
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(pgen_node) {
                break;
            }
        }
        assert!(state.is_completed(pgen_node));

        let edge = state
            .graph
            .graph
            .edge_weight(edge_idx)
            .expect("edge still exists");
        assert_eq!(edge.start_time, 0.0);
        assert!(
            !edge.finish_time.is_nan(),
            "completed edge should have a finish time"
        );
        assert_eq!(
            edge.finish_time,
            state.graph[pgen_node].finish_time().expect("pgen finished")
        );
    }

    #[test]
    fn t3_engineer_builds_abstract_goal() {
        let units = load_units();
        let mut state = SimulationState::new(
            &units,
            &[UnitKind::Commander, UnitKind::Engineer(TechLevel::T3)],
        );
        let eng_node = NodeId::new(1);
        let goal = Goal {
            tech_level: TechLevel::T4,
            mass_cost: 100.0,
            energy_cost: 1000.0,
            build_time: 100.0,
        };
        state
            .start_goal_project(goal, &[eng_node], &units)
            .expect("T3 engineer can start goal project");
        assert!(state.goal_project_active());

        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.goal_reached(&goal) {
                break;
            }
        }
        assert!(state.goal_reached(&goal));
    }

    #[test]
    fn t3_engineer_builds_monkeylord_in_expected_time() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let t3_eng = UnitKind::Engineer(TechLevel::T3);

        let mut state = SimulationState::new(&units, &[UnitKind::Commander, t3_eng.clone()]);
        let eng_node = NodeId::new(1);

        // Provide a huge, non-stalling economy so progress runs at full build power.
        state.economy.mass_storage = crate::quantities::Mass::from_raw(1_000_000.0);
        state.economy.energy_storage = crate::quantities::Energy::from_raw(10_000_000.0);
        state.economy.mass_storage_cap = crate::quantities::Mass::from_raw(1_000_000.0);
        state.economy.energy_storage_cap = crate::quantities::Energy::from_raw(10_000_000.0);
        state.economy.net_mass_income = crate::quantities::MassRate::from_raw(100_000.0);
        state.economy.net_energy_income = crate::quantities::EnergyRate::from_raw(1_000_000.0);

        let ml_node = state
            .start_project(&monkeylord, &[eng_node], &units)
            .expect("T3 engineer can build Monkeylord");

        let cost = units
            .build_cost(&monkeylord)
            .expect("Monkeylord has a cost");
        let build_power = units.def(&t3_eng).unwrap().build_rate();
        let expected_time = cost.build_time / build_power;

        let mut completed = Vec::new();
        for _ in 0..(expected_time.ceil() as usize + 100) {
            completed.extend(state.tick(&units, 1.0));
            if !completed.is_empty() {
                break;
            }
        }

        assert_eq!(completed.len(), 1, "Monkeylord should complete");
        assert_eq!(completed[0], ml_node);

        let finish_time = state.graph[ml_node]
            .finish_time()
            .expect("Monkeylord has a finish time");
        assert!(
            (finish_time - expected_time).abs() < 1e-6,
            "expected finish time ~{}, got {}",
            expected_time,
            finish_time
        );
    }

    #[test]
    fn builders_are_indivisible() {
        let units = load_units();

        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        let acu_node = NodeId::new(0);
        state
            .start_project(&UnitKind::Pgen(TechLevel::T1), &[acu_node], &units)
            .expect("ACU can build pgen");

        // ACU is busy, so starting another project with it must fail.
        let result = state.start_project(&UnitKind::Mex(TechLevel::T1), &[acu_node], &units);
        assert!(
            matches!(result, Err(GraphSimError::BuilderBusy(id)) if id == acu_node),
            "ACU should be busy"
        );
    }

    #[test]
    fn concurrent_projects_with_disjoint_builders() {
        let units = load_units();

        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        let acu_node = NodeId::new(0);
        let factory_node = state
            .start_project(&UnitKind::Factory(TechLevel::T1), &[acu_node], &units)
            .expect("ACU builds factory");

        // Tick until the factory completes.
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(factory_node) {
                break;
            }
        }
        assert!(state.is_completed(factory_node), "factory should finish");

        // Start two concurrent projects: factory builds an engineer, ACU builds
        // a pgen. Both use disjoint builder sets.
        let eng_node = state
            .start_project(&UnitKind::Engineer(TechLevel::T1), &[factory_node], &units)
            .expect("factory builds engineer");
        let pgen_node = state
            .start_project(&UnitKind::Pgen(TechLevel::T1), &[acu_node], &units)
            .expect("ACU builds pgen");

        assert!(
            state.idle_builders(&units).is_empty(),
            "all builders should be assigned"
        );

        // Both should make progress each tick.
        let before_eng = state.graph[eng_node].remaining_work().unwrap();
        let before_pgen = state.graph[pgen_node].remaining_work().unwrap();
        state.tick(&units, 1.0);
        assert!(state.graph[eng_node].remaining_work().unwrap() < before_eng);
        assert!(state.graph[pgen_node].remaining_work().unwrap() < before_pgen);

        // Finish both.
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(eng_node) && state.is_completed(pgen_node) {
                break;
            }
        }
        assert!(state.is_completed(eng_node), "engineer should finish");
        assert!(state.is_completed(pgen_node), "pgen should finish");
    }

    #[test]
    fn upgrade_t1_mex_to_t2_adds_new_node_and_updates_economy() {
        let units = load_units();
        let t1_mex = UnitKind::Mex(TechLevel::T1);
        let t2_mex = UnitKind::Mex(TechLevel::T2);

        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        let acu_node = NodeId::new(0);

        // Build a T1 mex.
        let mex_node = state
            .start_project(&t1_mex, &[acu_node], &units)
            .expect("ACU builds T1 mex");
        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(mex_node) {
                break;
            }
        }
        assert!(state.is_active(mex_node), "T1 mex should be active");
        assert!(
            state.graph[mex_node].finish_time().is_some(),
            "finished node has a finish time"
        );

        // Economy should include the T1 mex production.
        let income_with_t1 = state.economy.net_mass_income;
        assert!(income_with_t1 > 1.0, "T1 mex should add mass income");

        // Upgrade the mex to T2. A new node is added; the old node is retired.
        let upgraded_node = state
            .start_upgrade_project(&t2_mex, mex_node, &[acu_node], &units)
            .expect("ACU can upgrade the mex");
        assert!(
            !state.is_active(mex_node),
            "original mex node should no longer be active"
        );
        assert_eq!(
            state.graph[mex_node].replaced_by(),
            Some(upgraded_node),
            "original node should remember what replaced it"
        );
        assert!(
            state.graph[upgraded_node].is_upgrade(),
            "new node should be in an upgrade state"
        );
        assert_eq!(
            state.graph[upgraded_node].from_unit_id(),
            Some(&t1_mex),
            "upgrade should remember the original unit kind"
        );

        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(upgraded_node) {
                break;
            }
        }
        assert!(
            state.is_active(upgraded_node),
            "upgraded node should be active"
        );
        assert!(
            matches!(
                state.graph[upgraded_node].state,
                UnitNodeState::Upgraded { .. }
            ),
            "new node should finish in the Upgraded state"
        );
        assert!(
            !state.is_active(mex_node),
            "original mex node should stay retired after upgrade completes"
        );

        // Economy should now reflect only the T2 mex, which produces more mass.
        let income_after_upgrade = state.economy.net_mass_income;
        assert!(
            income_after_upgrade > income_with_t1,
            "T2 mex should produce more mass than T1 mex"
        );
    }

    #[test]
    fn energy_stall_reduces_mass_income() {
        let units = load_units();

        // Force an energy-stalled project by starting a huge drain with no
        // energy income. We do this by creating a fake project state manually.
        let mut state = SimulationState::new(&units, &[UnitKind::Commander]);
        state.economy.net_mass_income = crate::quantities::MassRate::from_raw(10.0);
        state.economy.net_energy_income = crate::quantities::EnergyRate::from_raw(0.0);
        state.economy.energy_storage = crate::quantities::Energy::from_raw(0.0);
        state.economy.mass_storage = crate::quantities::Mass::from_raw(0.0);

        let result = apply_tick_graph(0.0, 100.0, &state.economy, 1.0);
        assert!(result.energy_stalled);
        assert_eq!(result.scaled_net_mass_income, 0.0);
    }
}
