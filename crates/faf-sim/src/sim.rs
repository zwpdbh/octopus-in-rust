//! Build simulator for FAF build-order planning.
//!
//! This module combines two responsibilities:
//!
//! 1. **Economy derivation** — computing an [`EconomyState`] from a snapshot of
//!    owned units (production, storage, maintenance).
//! 2. **Graph-growth simulation** — the model from `tutorials/my_model.md` where
//!    nodes are built units, edges record builder assignments, and builders are
//!    indivisible (one target at a time). Multiple projects may run concurrently
//!    as long as they use disjoint builder sets.

use std::collections::HashSet;

use petgraph::graph::{DiGraph, NodeIndex};

use crate::economy::{
    apply_tick_graph, compute_drain, summarize_economy, EconomyState, RequestedBuildPower,
};
use crate::units::{BuildTargetStats, Unit, Units};

/// A single event in the simulated build timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildEvent {
    /// In-game seconds when the unit completed.
    pub time: f64,
    /// Blueprint id of the completed unit.
    pub unit_id: String,
    /// Display name for the completed unit.
    pub unit_name: String,
}

/// Derive an economy state by summing production, consumption, and storage
/// across units.
///
/// Net income is production minus maintenance consumption. It may be negative,
/// matching the in-game economy display.
pub fn derive_economy(units: &Units, unit_ids: &[&str]) -> EconomyState {
    let unit_refs: Vec<_> = unit_ids.iter().filter_map(|id| units.find(id)).collect();

    let mut mass_storage = 0.0;
    let mut energy_storage = 0.0;

    for unit in &unit_refs {
        if let Some(econ) = &unit.economy {
            mass_storage += econ.storage_mass.unwrap_or(0.0);
            energy_storage += econ.storage_energy.unwrap_or(0.0);
        }
    }

    let net = summarize_economy(units, unit_ids, &[]);

    EconomyState {
        net_mass_income: net.mass_per_second,
        net_energy_income: net.energy_per_second,
        mass_storage,
        energy_storage,
        mass_storage_cap: mass_storage,
        energy_storage_cap: energy_storage,
    }
}

/// Index type used by the build graph.
pub type GraphIndex = usize;

/// Opaque identifier for a node in the build graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub NodeIndex<GraphIndex>);

impl NodeId {
    /// Create a node id from a raw index.
    pub fn new(index: usize) -> Self {
        Self(NodeIndex::new(index))
    }

    /// Return the raw node index.
    pub fn index(&self) -> usize {
        self.0.index()
    }
}

impl From<usize> for NodeId {
    fn from(index: usize) -> Self {
        Self::new(index)
    }
}

/// Lifecycle state of a slot in the build graph.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitNodeState {
    /// The slot is currently being worked on: either initial construction or an
    /// upgrade. It is not yet contributing to the economy or acting as a builder.
    Building(BuildingUnitState),
    /// The slot has finished its current incarnation and is active.
    Finished(FinishedUnitState),
}

/// State of a slot that is currently under active construction or upgrade.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildingUnitState {
    /// Initial construction of the unit in this slot.
    Constructing {
        /// When construction began.
        start: f64,
        /// Builders capable of building the target.
        started_by: Vec<NodeId>,
        /// Additional builders assisting construction.
        assisted_by: Vec<NodeId>,
    },
    /// Upgrade of an existing unit in this slot to a higher-tier version.
    Upgrading {
        /// When the upgrade began.
        start: f64,
        /// The unit id this slot is being upgraded from.
        from_unit_id: String,
        /// Builders capable of building the upgrade target.
        started_by: Vec<NodeId>,
        /// Additional builders assisting the upgrade.
        assisted_by: Vec<NodeId>,
    },
}

/// State of a slot that has finished its current incarnation.
#[derive(Debug, Clone, PartialEq)]
pub enum FinishedUnitState {
    /// Built from scratch.
    Constructed {
        /// When construction began.
        start_time: f64,
        /// When construction completed.
        finish_time: f64,
    },
    /// Reached by upgrading an earlier unit in the same slot.
    Upgraded {
        /// When the upgrade began.
        start_time: f64,
        /// When the upgrade completed.
        finish_time: f64,
        /// The unit id before this upgrade completed.
        from_unit_id: String,
    },
}

/// One built unit in the growing build graph.
#[derive(Debug, Clone)]
pub struct UnitNode {
    /// Stable node identifier.
    pub id: NodeId,
    /// Current blueprint id of the unit in this slot.
    pub unit_id: String,
    /// Lifecycle state of this slot.
    pub state: UnitNodeState,
}

impl UnitNode {
    /// True if this slot has finished construction or upgrade.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, UnitNodeState::Finished(_))
    }

    /// True if this slot is finished and currently active.
    pub fn is_active(&self) -> bool {
        self.is_finished()
    }

    /// The finish time of this slot, if it has finished.
    pub fn finish_time(&self) -> Option<f64> {
        match &self.state {
            UnitNodeState::Finished(FinishedUnitState::Constructed { finish_time, .. }) => {
                Some(*finish_time)
            }
            UnitNodeState::Finished(FinishedUnitState::Upgraded { finish_time, .. }) => {
                Some(*finish_time)
            }
            _ => None,
        }
    }

    /// True if this slot was reached by upgrading an earlier unit.
    pub fn is_upgrade(&self) -> bool {
        matches!(
            self.state,
            UnitNodeState::Building(BuildingUnitState::Upgrading { .. })
                | UnitNodeState::Finished(FinishedUnitState::Upgraded { .. })
        )
    }

    /// The unit id this slot upgraded from, if any.
    pub fn from_unit_id(&self) -> Option<&str> {
        match &self.state {
            UnitNodeState::Building(BuildingUnitState::Upgrading { from_unit_id, .. }) => {
                Some(from_unit_id)
            }
            UnitNodeState::Finished(FinishedUnitState::Upgraded { from_unit_id, .. }) => {
                Some(from_unit_id)
            }
            _ => None,
        }
    }
}

/// A builder assignment edge in the build graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildEdge {
    /// When this builder started contributing to the target project.
    pub start_time: f64,
    /// When this builder finished contributing to the target project. `NaN`
    /// until the target completes.
    pub finish_time: f64,
}

/// The growing directed graph of built units and builder assignments.
#[derive(Debug, Clone)]
pub struct BuildGraph {
    /// Underlying petgraph directed graph. Nodes are [`UnitNode`]s and edges
    /// are builder assignments (`builder -> built unit`).
    pub graph: DiGraph<UnitNode, BuildEdge, GraphIndex>,
}

impl Default for BuildGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::with_capacity(0, 0),
        }
    }
}

impl std::ops::Index<NodeId> for BuildGraph {
    type Output = UnitNode;
    fn index(&self, id: NodeId) -> &Self::Output {
        &self.graph[id.0]
    }
}

impl std::ops::IndexMut<NodeId> for BuildGraph {
    fn index_mut(&mut self, id: NodeId) -> &mut Self::Output {
        &mut self.graph[id.0]
    }
}

/// An ongoing build: a target unit and the builders currently working on it.
#[derive(Debug, Clone)]
pub struct OngoingBuild {
    /// Node id of the unit being built or upgraded.
    pub target_node: NodeId,
    /// Builder nodes assigned to this build. Builders are indivisible.
    pub builders: Vec<NodeId>,
    /// Remaining work in blueprint `BuildTime` units.
    pub remaining_work: f64,
    /// Time when this build started.
    pub start_time: f64,
}

/// Errors that can occur when manipulating a `GraphState`.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphSimError {
    /// A requested builder is already assigned to another project.
    BuilderBusy(NodeId),
    /// No builders were provided for a new project.
    NoBuilders,
    /// The builder cannot build the requested target.
    CannotBuild { builder: String, target: String },
    /// The target unit cannot be built at all.
    NotBuildable(String),
    /// The requested active project was not found.
    ProjectNotFound,
}

impl std::fmt::Display for GraphSimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphSimError::BuilderBusy(id) => write!(f, "builder {} is busy", id.index()),
            GraphSimError::NoBuilders => write!(f, "no builders assigned to project"),
            GraphSimError::CannotBuild { builder, target } => {
                write!(f, "builder {} cannot build {}", builder, target)
            }
            GraphSimError::NotBuildable(id) => write!(f, "unit {} is not buildable", id),
            GraphSimError::ProjectNotFound => write!(f, "active project not found"),
        }
    }
}

impl std::error::Error for GraphSimError {}

/// Mutable simulation state for the graph model.
#[derive(Debug, Clone)]
pub struct GraphState {
    /// Current simulation time in seconds.
    pub time: f64,
    /// The build graph.
    pub graph: BuildGraph,
    /// Current economy state.
    pub economy: EconomyState,
    /// Builds currently under construction.
    pub active_projects: Vec<OngoingBuild>,
    /// Completed build events in chronological order.
    pub events: Vec<BuildEvent>,
}

/// True if the node represents an active builder unit.
fn is_builder_node(node_id: NodeId, graph: &BuildGraph, units: &Units) -> bool {
    let node = &graph[node_id];
    if !node.is_active() {
        return false;
    }
    units.find(&node.unit_id).is_some_and(|u| {
        (u.has_category("COMMANDER")
            || u.has_category("ENGINEER")
            || u.has_category("FACTORY"))
            && u.builder_capability().is_some()
    })
}

/// Build power contributed by a single active builder node.
pub(crate) fn builder_power(node_id: NodeId, graph: &BuildGraph, units: &Units) -> f64 {
    let node = &graph[node_id];
    if !node.is_active() {
        return 0.0;
    }
    let Some(unit) = units.find(&node.unit_id) else {
        return 0.0;
    };
    unit.builder_capability()
        .map(|cap| cap.build_rate)
        .unwrap_or(0.0)
}

impl GraphState {
    /// Create a new simulation state from the given starting unit ids.
    ///
    /// All starting units are treated as already completed at time 0. Any
    /// builders among them are added to `idle_builders`.
    pub fn new(units: &Units, starting_unit_ids: &[&str]) -> Self {
        let mut graph = BuildGraph::default();

        for (i, id) in starting_unit_ids.iter().enumerate() {
            let node_id = NodeId::new(i);
            graph.graph.add_node(UnitNode {
                id: node_id,
                unit_id: id.to_string(),
                state: UnitNodeState::Finished(FinishedUnitState::Constructed {
                    start_time: 0.0,
                    finish_time: 0.0,
                }),
            });
        }

        let economy = derive_economy(units, starting_unit_ids);

        Self {
            time: 0.0,
            graph,
            economy,
            active_projects: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Return the builder nodes that are currently idle and available for new
    /// work.
    ///
    /// This is derived from `graph` (active builder nodes) and `active_projects`
    /// (builders currently assigned to a project). It is computed on demand so
    /// there is only one source of truth for builder availability.
    pub fn idle_builders(&self, units: &Units) -> Vec<NodeId> {
        let busy: HashSet<NodeId> = self
            .active_projects
            .iter()
            .flat_map(|p| p.builders.iter())
            .copied()
            .collect();

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

    /// True if an active unit with the given blueprint id has been completed.
    pub fn has_completed_unit(&self, unit_id: &str) -> bool {
        self.graph
            .graph
            .node_weights()
            .any(|n| n.is_active() && n.unit_id.eq_ignore_ascii_case(unit_id))
    }

    /// True if the given goal unit has been completed.
    pub fn goal_reached(&self, goal_id: &str) -> bool {
        self.has_completed_unit(goal_id)
    }

    /// Return the blueprint ids of all units currently under construction.
    pub fn active_target_unit_ids(&self) -> HashSet<String> {
        self.active_projects
            .iter()
            .map(|p| self.graph[p.target_node].unit_id.to_ascii_uppercase())
            .collect()
    }

    /// Count how many active units belong to the given category.
    pub fn count_active_by_category(&self, units: &Units, category: &str) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter_map(|n| units.find(&n.unit_id))
            .filter(|u| u.has_category(category))
            .count()
    }

    /// Return the blueprint data for every active unit in the graph.
    pub fn active_unit_blueprints<'a>(&'a self, units: &'a Units) -> Vec<&'a Unit> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter_map(|n| units.find(&n.unit_id))
            .collect()
    }

    /// Total build power of all active builders, including idle builders and
    /// builders currently assigned to active projects.
    pub fn total_active_build_power(&self, units: &Units) -> f64 {
        self.idle_builders(units)
            .iter()
            .chain(self.active_projects.iter().flat_map(|p| p.builders.iter()))
            .map(|&b| builder_power(b, &self.graph, units))
            .sum()
    }

    /// Re-derive the economy from all active units.
    pub fn rebuild_economy(&mut self, units: &Units) {
        let active_ids: Vec<&str> = self
            .graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .map(|n| n.unit_id.as_str())
            .collect();
        self.economy = derive_economy(units, &active_ids);
    }

    /// Estimate the remaining time until `goal` is completed from this state.
    ///
    /// `chain_unit_ids` lists the prerequisite units that still need to be built
    /// before the goal. The estimate aggregates their remaining cost and work,
    /// then uses [`EconomyState::estimate_remaining_time`] to model how income,
    /// storage, and build power interact.
    ///
    /// This is a heuristic, not an exact simulation. Lower is better.
    pub fn estimate_remaining_time_to_goal(
        &self,
        goal_id: &str,
        chain_unit_ids: &[String],
        units: &Units,
    ) -> f64 {
        let mut total_mass = 0.0;
        let mut total_energy = 0.0;
        let mut total_work = 0.0;

        for id in chain_unit_ids {
            if self.has_completed_unit(id) {
                continue;
            }
            if let Some(stats) = units.build_cost(id) {
                total_mass += stats.build_cost_mass;
                total_energy += stats.build_cost_energy;
                total_work += stats.build_time;
            }
        }

        if !self.has_completed_unit(goal_id) {
            if let Some(stats) = units.build_cost(goal_id) {
                total_mass += stats.build_cost_mass;
                total_energy += stats.build_cost_energy;
                total_work += stats.build_time;
            }
        }

        self.economy.estimate_remaining_time(
            BuildTargetStats {
                build_cost_mass: total_mass,
                build_cost_energy: total_energy,
                build_time: total_work,
            },
            self.total_active_build_power(units),
        )
    }

    /// Validate that every builder in `builders` is idle and is a real builder.
    fn validate_builders(
        &self,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<(), GraphSimError> {
        if builders.is_empty() {
            return Err(GraphSimError::NoBuilders);
        }

        let busy: HashSet<NodeId> = self
            .active_projects
            .iter()
            .flat_map(|p| p.builders.iter())
            .copied()
            .collect();

        for &builder in builders {
            if busy.contains(&builder) {
                return Err(GraphSimError::BuilderBusy(builder));
            }
            if !is_builder_node(builder, &self.graph, units) {
                return Err(GraphSimError::CannotBuild {
                    builder: self.graph[builder].unit_id.clone(),
                    target: "(not a builder)".to_string(),
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
        target_id: &str,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, units)?;

        let stats = units
            .build_cost(target_id)
            .ok_or_else(|| GraphSimError::NotBuildable(target_id.to_string()))?;

        let (started_by, assisted_by) = self.split_builders(builders, target_id, units);
        if started_by.is_empty() {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph[*b].unit_id.clone())
                    .unwrap_or_default(),
                target: target_id.to_string(),
            });
        }

        let node_id = NodeId::new(self.graph.graph.node_count());
        self.graph.graph.add_node(UnitNode {
            id: node_id,
            unit_id: target_id.to_string(),
            state: UnitNodeState::Building(BuildingUnitState::Constructing {
                start: self.time,
                started_by: started_by.clone(),
                assisted_by: assisted_by.clone(),
            }),
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

        self.active_projects.push(OngoingBuild {
            target_node: node_id,
            builders: builders.to_vec(),
            remaining_work: stats.build_time,
            start_time: self.time,
        });

        Ok(node_id)
    }

    /// Split builders into those capable of building the target and assistants.
    fn split_builders(
        &self,
        builders: &[NodeId],
        target_id: &str,
        units: &Units,
    ) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut started_by = Vec::new();
        let mut assisted_by = Vec::new();
        for &builder in builders {
            let builder_unit_id = &self.graph[builder].unit_id;
            if units.can_build(builder_unit_id, target_id) {
                started_by.push(builder);
            } else {
                assisted_by.push(builder);
            }
        }
        (started_by, assisted_by)
    }

    /// Start an upgrade of `old_node` to `target` using the given idle builders.
    ///
    /// The same physical slot is reused: `old_node` moves into the `Upgrading`
    /// state and its `unit_id` becomes the target blueprint. On completion the
    /// slot finishes as `Finished(Upgraded { from_unit_id: old_id })`.
    ///
    /// `upgrade_cost` provides the mass, energy, and build-time required for the
    /// upgrade step; it is separate from `target.build_target_stats()` because
    /// upgrades in FAF have their own costs.
    pub fn start_upgrade_project(
        &mut self,
        target_id: &str,
        old_node: NodeId,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, units)?;

        let old_unit_id = self.graph[old_node].unit_id.clone();
        let (target, upgrade_cost) = units
            .upgrade_target(&old_unit_id)
            .ok_or_else(|| GraphSimError::NotBuildable(target_id.to_string()))?;
        if target.id != target_id {
            return Err(GraphSimError::NotBuildable(target_id.to_string()));
        }
        let stats = upgrade_cost.to_build_target_stats();

        let (started_by, assisted_by) = self.split_builders(builders, target_id, units);
        if started_by.is_empty() {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph[*b].unit_id.clone())
                    .unwrap_or_default(),
                target: target_id.to_string(),
            });
        }

        self.graph[old_node].unit_id = target_id.to_string();
        self.graph[old_node].state = UnitNodeState::Building(BuildingUnitState::Upgrading {
            start: self.time,
            from_unit_id: old_unit_id.clone(),
            started_by: started_by.clone(),
            assisted_by: assisted_by.clone(),
        });

        for &builder in builders {
            self.graph.graph.add_edge(
                builder.0,
                old_node.0,
                BuildEdge {
                    start_time: self.time,
                    finish_time: f64::NAN,
                },
            );
        }

        self.active_projects.push(OngoingBuild {
            target_node: old_node,
            builders: builders.to_vec(),
            remaining_work: stats.build_time,
            start_time: self.time,
        });

        Ok(old_node)
    }

    /// Assign additional idle `builders` to an already active project.
    ///
    /// Assisting builders do not need to be capable of building the target;
    /// they only need to be real builders.
    pub fn assist_project(
        &mut self,
        project_index: usize,
        builders: &[NodeId],
        units: &Units,
    ) -> Result<(), GraphSimError> {
        self.validate_builders(builders, units)?;

        let project = self
            .active_projects
            .get(project_index)
            .ok_or(GraphSimError::ProjectNotFound)?;

        for &builder in builders {
            self.graph.graph.add_edge(
                builder.0,
                project.target_node.0,
                BuildEdge {
                    start_time: self.time,
                    finish_time: f64::NAN,
                },
            );
        }

        self.active_projects[project_index]
            .builders
            .extend(builders.iter().copied());
        Ok(())
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Returns the node ids of any units that completed during this tick.
    pub fn tick(&mut self, units: &Units, dt: f64) -> Vec<NodeId> {
        if dt <= 0.0 {
            return Vec::new();
        }

        if self.active_projects.is_empty() {
            self.apply_idle_income(dt);
            self.time += dt;
            return Vec::new();
        }

        // Compute total drain across all active projects.
        let mut total_mass_drain = 0.0;
        let mut total_energy_drain = 0.0;
        let mut project_powers: Vec<f64> = Vec::with_capacity(self.active_projects.len());

        for project in &self.active_projects {
            let target_id = &self.graph[project.target_node].unit_id;
            let Some(stats) = units.build_cost(target_id) else {
                project_powers.push(0.0);
                continue;
            };
            let power: f64 = project
                .builders
                .iter()
                .map(|&b| builder_power(b, &self.graph, units))
                .sum();
            project_powers.push(power);
            let Some(drain) = compute_drain(&stats, RequestedBuildPower(power)) else {
                continue;
            };
            total_mass_drain += drain.mass_per_second;
            total_energy_drain += drain.energy_per_second;
        }

        let tick_result = apply_tick_graph(total_mass_drain, total_energy_drain, &self.economy, dt);

        // Apply progress and record exact finish times for completed projects.
        let mut completed_nodes = Vec::new();
        for (i, project) in self.active_projects.iter_mut().enumerate() {
            let power = project_powers[i];
            if power <= 0.0 {
                continue;
            }
            let progress = tick_result.effective_factor * power * dt;
            let work_before = project.remaining_work;
            project.remaining_work -= progress;
            if project.remaining_work <= 0.0 {
                project.remaining_work = 0.0;
                let fraction = if progress > 0.0 {
                    (work_before / progress).min(1.0)
                } else {
                    1.0
                };
                let finish_time = self.time + fraction * dt;

                // Transition the target node to its finished state.
                {
                    let node = &mut self.graph[project.target_node];
                    let (start_time, from_unit_id) = match &node.state {
                        UnitNodeState::Building(BuildingUnitState::Constructing {
                            start, ..
                        }) => (*start, None),
                        UnitNodeState::Building(BuildingUnitState::Upgrading {
                            start,
                            from_unit_id,
                            ..
                        }) => (*start, Some(from_unit_id.clone())),
                        _ => (self.time, None),
                    };
                    node.state = match from_unit_id {
                        Some(from_unit_id) => {
                            UnitNodeState::Finished(FinishedUnitState::Upgraded {
                                start_time,
                                finish_time,
                                from_unit_id,
                            })
                        }
                        None => UnitNodeState::Finished(FinishedUnitState::Constructed {
                            start_time,
                            finish_time,
                        }),
                    };
                }

                // Record the finish time on every builder assignment edge.
                for &builder in &project.builders {
                    if let Some(edge_idx) =
                        self.graph.graph.find_edge(builder.0, project.target_node.0)
                    {
                        if let Some(edge) = self.graph.graph.edge_weight_mut(edge_idx) {
                            edge.finish_time = finish_time;
                        }
                    }
                }

                completed_nodes.push(project.target_node);
            }
        }

        // Update storage. Net income remains the base value; scaling is
        // recomputed each tick based on current conditions.
        self.economy.mass_storage = tick_result.new_mass_storage;
        self.economy.energy_storage = tick_result.new_energy_storage;

        self.time += dt;

        // Complete projects in reverse order so we can remove safely.
        let mut completed_indices: Vec<usize> = (0..self.active_projects.len())
            .filter(|&i| self.active_projects[i].remaining_work <= 0.0)
            .collect();
        completed_indices.sort_by(|a, b| b.cmp(a));

        for i in completed_indices {
            let project = self.active_projects.remove(i);
            let node = &self.graph[project.target_node];
            let unit_id = node.unit_id.clone();
            let finish_time = match &node.state {
                UnitNodeState::Finished(FinishedUnitState::Constructed { finish_time, .. }) => {
                    *finish_time
                }
                UnitNodeState::Finished(FinishedUnitState::Upgraded { finish_time, .. }) => {
                    *finish_time
                }
                _ => self.time,
            };

            self.events.push(BuildEvent {
                time: finish_time,
                unit_id: unit_id.clone(),
                unit_name: units
                    .find(&unit_id)
                    .map(|u| u.display_name())
                    .unwrap_or_else(|| unit_id.clone()),
            });
        }

        // Re-derive economy from all active (completed and not replaced) units.
        if !completed_nodes.is_empty() {
            self.rebuild_economy(units);
        }

        completed_nodes
    }

    /// Collect income for one tick with no active projects.
    fn apply_idle_income(&mut self, dt: f64) {
        self.economy.mass_storage = (self.economy.mass_storage + self.economy.net_mass_income * dt)
            .min(self.economy.mass_storage_cap)
            .max(0.0);
        self.economy.energy_storage = (self.economy.energy_storage
            + self.economy.net_energy_income * dt)
            .min(self.economy.energy_storage_cap)
            .max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{DataIndex, default_upgrade_table, Units, UpgradeCost};

    fn load_units() -> Units {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn acu_starting_economy() {
        let units = load_units();
        let state = derive_economy(&units, &["URL0001"]);

        assert!((state.net_mass_income - 1.0).abs() < 1e-9);
        assert!((state.net_energy_income - 20.0).abs() < 1e-9);
        assert!((state.mass_storage - 650.0).abs() < 1e-9);
        assert!((state.energy_storage - 3900.0).abs() < 1e-9);
    }

    #[test]
    fn derive_economy_subtracts_maintenance() {
        let units = load_units();
        let state = derive_economy(&units, &["URL0001", "URB1103"]);

        // ACU: +1 mass/s, +20 energy/s. T1 mex: +2 mass/s, -2 energy/s maintenance.
        assert!((state.net_mass_income - 3.0).abs() < 1e-9);
        assert!((state.net_energy_income - 18.0).abs() < 1e-9);
    }

    #[test]
    fn acu_builds_t1_pgen() {
        let units = load_units();
        let acu = units.find("URL0001").expect("ACU exists");
        let pgen = units.find("URB1101").expect("T1 pgen exists");

        let mut state = GraphState::new(&units, &["URL0001"]);
        let acu_node = NodeId::new(0);
        state
            .start_project("URB1101", &[acu_node], &units)
            .expect("ACU can build T1 pgen");

        let acu_rate = acu.builder_capability().unwrap().build_rate;
        let expected_ticks = (pgen.build_target_stats().unwrap().build_time / acu_rate).ceil();
        let mut completed = Vec::new();
        for _ in 0..(expected_ticks as usize + 5) {
            completed.extend(state.tick(&units, 1.0));
            if !completed.is_empty() {
                break;
            }
        }

        assert_eq!(completed.len(), 1);
        assert_eq!(state.graph[completed[0]].unit_id, "URB1101");
        assert!(state.time > 0.0);
        assert!(state.idle_builders(&units).contains(&acu_node));
    }

    #[test]
    fn build_edge_records_interval() {
        let units = load_units();

        let mut state = GraphState::new(&units, &["URL0001"]);
        let acu_node = NodeId::new(0);
        let pgen_node = state
            .start_project("URB1101", &[acu_node], &units)
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
    fn builders_are_indivisible() {
        let units = load_units();

        let mut state = GraphState::new(&units, &["URL0001"]);
        let acu_node = NodeId::new(0);
        state
            .start_project("URB1101", &[acu_node], &units)
            .expect("ACU can build pgen");

        // ACU is busy, so starting another project with it must fail.
        let result = state.start_project("URB1103", &[acu_node], &units);
        assert!(
            matches!(result, Err(GraphSimError::BuilderBusy(id)) if id == acu_node),
            "ACU should be busy"
        );
    }

    #[test]
    fn concurrent_projects_with_disjoint_builders() {
        let units = load_units();

        let mut state = GraphState::new(&units, &["URL0001"]);
        let acu_node = NodeId::new(0);
        let factory_node = state
            .start_project("URB0101", &[acu_node], &units)
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
            .start_project("URL0105", &[factory_node], &units)
            .expect("factory builds engineer");
        let pgen_node = state
            .start_project("URB1101", &[acu_node], &units)
            .expect("ACU builds pgen");

        assert_eq!(state.active_projects.len(), 2);
        assert!(
            state.idle_builders(&units).is_empty(),
            "all builders should be assigned"
        );

        // Both should make progress each tick.
        let before0 = state.active_projects[0].remaining_work;
        let before1 = state.active_projects[1].remaining_work;
        state.tick(&units, 1.0);
        assert!(state.active_projects[0].remaining_work < before0);
        assert!(state.active_projects[1].remaining_work < before1);

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
    fn upgrade_reuses_slot_and_updates_economy() {
        // Use a custom upgrade table so the ACU can build the upgrade target.
        // This keeps the test focused on slot reuse and economy update.
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        let index: DataIndex =
            serde_json::from_str(json).expect("embedded index should parse");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");
        let pgen_stats = pgen.build_target_stats().expect("pgen has stats");
        let mut upgrade_table = default_upgrade_table();
        upgrade_table.insert(
            "URB1103",
            "URB1101",
            UpgradeCost {
                mass: pgen_stats.build_cost_mass,
                energy: pgen_stats.build_cost_energy,
                build_time: pgen_stats.build_time,
            },
        );
        let units = Units::with_upgrade_table(index, upgrade_table);
        let t1_mex = units.find("URB1103").expect("T1 mex exists");

        let mut state = GraphState::new(&units, &["URL0001"]);
        let acu_node = NodeId::new(0);

        // Build a T1 mex.
        let mex_node = state
            .start_project("URB1103", &[acu_node], &units)
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

        // Upgrade the mex slot to a pgen. The same node id is reused.
        state
            .start_upgrade_project("URB1101", mex_node, &[acu_node], &units)
            .expect("ACU can upgrade the mex slot");
        assert!(
            state.graph[mex_node].is_upgrade(),
            "slot should be in an upgrade state"
        );
        assert_eq!(
            state.graph[mex_node].from_unit_id(),
            Some(t1_mex.id.as_str()),
            "upgrade should remember the original unit id"
        );

        for _ in 0..1000 {
            state.tick(&units, 1.0);
            if state.is_completed(mex_node) {
                break;
            }
        }
        assert!(state.is_active(mex_node), "upgraded slot should be active");
        assert!(
            matches!(
                state.graph[mex_node].state,
                UnitNodeState::Finished(FinishedUnitState::Upgraded { .. })
            ),
            "slot should finish in the Upgraded state"
        );

        // Economy should now reflect the pgen, not the mex.
        let income_after_upgrade = state.economy.net_mass_income;
        assert!(
            (income_after_upgrade - 1.0).abs() < 1e-3,
            "upgrading mex to pgen should leave only ACU mass income"
        );
    }

    #[test]
    fn energy_stall_reduces_mass_income() {
        let units = load_units();

        // Force an energy-stalled project by starting a huge drain with no
        // energy income. We do this by creating a fake project state manually.
        let mut state = GraphState::new(&units, &["URL0001"]);
        state.economy.net_mass_income = 10.0;
        state.economy.net_energy_income = 0.0;
        state.economy.energy_storage = 0.0;
        state.economy.mass_storage = 0.0;

        let result = apply_tick_graph(0.0, 100.0, &state.economy, 1.0);
        assert!(result.energy_stalled);
        assert_eq!(result.scaled_net_mass_income, 0.0);
    }
}
