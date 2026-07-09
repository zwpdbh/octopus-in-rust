//! Graph primitives and unit/build-order state for the simulation model.
//!
//! Nodes are built units, edges record builder assignments, and builders are
//! indivisible (one target at a time). Multiple projects may run concurrently
//! as long as they use disjoint builder sets.
//!
//! [`UnitGraph`] is the unit-specific counterpart to [`EcoEngine`]. It owns the
//! build graph, adjacency tracker, build events, and unit knowledge, but it does
//! not own the economy state. Instead it derives an [`EconomyState`] from active
//! units and mutates an externally provided economy during [`UnitGraph::tick`].

use std::collections::HashSet;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use crate::economy::{
    apply_tick_graph, compute_drain, EcoConsumer, EcoFlow, EcoProducer, EconomyState,
    RequestedBuildPower,
};
use crate::engine::adjacency::{production_multiplier, AdjacencyKind, AdjacencyTracker};
use crate::quantities::{Energy, EnergyRate, Mass, MassRate, Time};
use crate::units::{UnitDef, UnitKind, Units};

/// A single event in the simulated build timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildEvent {
    /// In-game seconds when the unit completed.
    pub time: f64,
    /// Abstract kind of the completed unit.
    pub unit_id: UnitKind,
    /// Display name for the completed unit.
    pub unit_name: String,
    /// Node id of the completed unit in the build graph. This lets visualisers
    /// correlate an event with the builder assignments that produced it.
    pub node_id: NodeId,
}

/// Derive an economy state by summing production, consumption, and storage
/// across units.
///
/// Net income is production minus maintenance consumption. It may be negative,
/// matching the in-game economy display.
pub fn derive_economy(units: &Units, unit_kinds: &[UnitKind]) -> EconomyState {
    let defs: Vec<&UnitDef> = unit_kinds.iter().filter_map(|k| units.def(k)).collect();

    let mut mass_storage = Mass::zero();
    let mut energy_storage = Energy::zero();

    for def in &defs {
        mass_storage = mass_storage + Mass::from_raw(def.mass_storage());
        energy_storage = energy_storage + Energy::from_raw(def.energy_storage());
    }

    let production: EcoFlow = defs.iter().map(|d| d.production()).sum();
    let maintenance: EcoFlow = defs.iter().map(|d| d.consumption()).sum();
    let net = EcoFlow::net(&production, &maintenance);

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
    /// Initial construction of the unit in this slot.
    /// It is not yet contributing to the economy or acting as a builder.
    Constructing {
        /// When construction began.
        start: f64,
        /// Remaining work in blueprint build-time units.
        remaining_work: f64,
        /// Builders capable of building the target.
        started_by: Vec<NodeId>,
        /// Additional builders assisting construction.
        assisted_by: Vec<NodeId>,
    },
    /// Upgrade of an existing unit in this slot to a higher-tier version.
    /// It is not yet contributing to the economy or acting as a builder.
    Upgrading {
        /// When the upgrade began.
        start: f64,
        /// Remaining work in blueprint build-time units.
        remaining_work: f64,
        /// The unit kind this slot is being upgraded from.
        from_unit_id: UnitKind,
        /// Builders capable of building the upgrade target.
        started_by: Vec<NodeId>,
        /// Additional builders assisting the upgrade.
        assisted_by: Vec<NodeId>,
    },
    /// The slot has finished its current incarnation and is active.
    Constructed {
        /// When construction began.
        start_time: f64,
        /// When construction completed.
        finish_time: f64,
    },
    /// Reached by upgrading an earlier unit into this new slot.
    Upgraded {
        /// When the upgrade began.
        start_time: f64,
        /// When the upgrade completed.
        finish_time: f64,
        /// The unit kind before this upgrade completed.
        from_unit_id: UnitKind,
    },
    /// The slot used to hold a finished unit but has been replaced by an upgrade
    /// into `into`. It no longer contributes to the economy or acts as a builder.
    Replaced {
        /// When the original unit began construction.
        start_time: f64,
        /// When the original unit finished construction.
        finish_time: f64,
        /// The node id that now holds the upgraded unit.
        into: NodeId,
    },
}

/// One built unit in the growing build graph.
#[derive(Debug, Clone)]
pub struct UnitNode {
    /// Stable node identifier.
    pub id: NodeId,
    /// Current abstract kind of the unit in this slot.
    pub unit_id: UnitKind,
    /// Lifecycle state of this slot.
    pub state: UnitNodeState,
}

impl UnitNode {
    /// True if this slot has finished construction or upgrade.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.state,
            UnitNodeState::Constructed { .. }
                | UnitNodeState::Upgraded { .. }
                | UnitNodeState::Replaced { .. }
        )
    }

    /// True if this slot is finished and currently active.
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            UnitNodeState::Constructed { .. } | UnitNodeState::Upgraded { .. }
        )
    }

    /// The finish time of this slot, if it has finished or been replaced.
    pub fn finish_time(&self) -> Option<f64> {
        match &self.state {
            UnitNodeState::Constructed { finish_time, .. } => Some(*finish_time),
            UnitNodeState::Upgraded { finish_time, .. } => Some(*finish_time),
            UnitNodeState::Replaced { finish_time, .. } => Some(*finish_time),
            _ => None,
        }
    }

    /// The remaining work of this slot, if it is under construction or upgrade.
    pub fn remaining_work(&self) -> Option<f64> {
        match &self.state {
            UnitNodeState::Constructing { remaining_work, .. } => Some(*remaining_work),
            UnitNodeState::Upgrading { remaining_work, .. } => Some(*remaining_work),
            _ => None,
        }
    }

    /// True if this slot was reached by upgrading an earlier unit.
    pub fn is_upgrade(&self) -> bool {
        matches!(
            self.state,
            UnitNodeState::Upgrading { .. } | UnitNodeState::Upgraded { .. }
        )
    }

    /// The node id this slot was replaced by, if any.
    pub fn replaced_by(&self) -> Option<NodeId> {
        match &self.state {
            UnitNodeState::Replaced { into, .. } => Some(*into),
            _ => None,
        }
    }

    /// The unit kind this slot upgraded from, if any.
    pub fn from_unit_id(&self) -> Option<&UnitKind> {
        match &self.state {
            UnitNodeState::Upgrading { from_unit_id, .. } => Some(from_unit_id),
            UnitNodeState::Upgraded { from_unit_id, .. } => Some(from_unit_id),
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

/// Errors that can occur when manipulating the build graph.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphSimError {
    /// A requested builder is already assigned to another project.
    BuilderBusy(NodeId),
    /// No builders were provided for a new project.
    NoBuilders,
    /// The builder cannot build the requested target.
    CannotBuild { builder: UnitKind, target: UnitKind },
    /// The target unit cannot be built at all.
    NotBuildable(UnitKind),
    /// The requested active project was not found.
    ProjectNotFound,
    /// An abstract goal project is already active.
    GoalProjectActive,
}

impl std::fmt::Display for GraphSimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphSimError::BuilderBusy(id) => write!(f, "builder {} is busy", id.index()),
            GraphSimError::NoBuilders => write!(f, "no builders assigned to project"),
            GraphSimError::CannotBuild { builder, target } => {
                write!(f, "builder {:?} cannot build {:?}", builder, target)
            }
            GraphSimError::NotBuildable(id) => write!(f, "unit {:?} is not buildable", id),
            GraphSimError::ProjectNotFound => write!(f, "active project not found"),
            GraphSimError::GoalProjectActive => write!(f, "goal project is already active"),
        }
    }
}

impl std::error::Error for GraphSimError {}

/// Build power contributed by a single active builder node.
pub fn builder_power(node_id: NodeId, graph: &BuildGraph, units: &Units) -> f64 {
    let node = &graph[node_id];
    if !node.is_active() {
        return 0.0;
    }
    units.def(&node.unit_id).map_or(0.0, |d| d.build_rate())
}

/// Unit-specific build-order state.
///
/// `UnitGraph` owns the built-unit graph, adjacency bonuses, completed build
/// events, and the unit knowledge repository. It does not own the economy
/// state; instead it computes one with [`UnitGraph::derive_economy`] and ticks
/// an externally provided [`EconomyState`] with [`UnitGraph::tick`].
#[derive(Debug, Clone)]
pub struct UnitGraph {
    /// Current simulation time in seconds.
    pub time: f64,
    /// The build graph.
    pub graph: BuildGraph,
    /// Adjacency bonuses for mass and energy production.
    pub adjacency: AdjacencyTracker,
    /// Completed build events in chronological order.
    pub events: Vec<BuildEvent>,
    /// Unit knowledge repository.
    pub units: Units,
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

impl UnitGraph {
    /// Create a new unit graph from the given starting unit kinds.
    ///
    /// All starting units are treated as already completed at time 0.
    pub fn new(starting_units: &[UnitKind], units: Units) -> Self {
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

        Self {
            time: 0.0,
            graph,
            adjacency,
            events: Vec::new(),
            units,
        }
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

    /// Return the builder nodes that are currently idle and available for new
    /// work.
    pub fn idle_builders(&self) -> Vec<NodeId> {
        let busy = self.busy_builders();

        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter(|n| is_builder_node(n.id, &self.graph, &self.units))
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

    /// True if the unit represented by `node_id` has finished construction or
    /// upgrade.
    pub fn is_completed(&self, node_id: NodeId) -> bool {
        self.graph
            .graph
            .node_weight(node_id.0)
            .is_some_and(|n| n.is_finished())
    }

    /// True if the unit represented by `node_id` is active.
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

    /// Count how many active units have the given kind.
    pub fn count_active_by_kind(&self, kind: &UnitKind) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active() && n.unit_id == *kind)
            .count()
    }

    /// Count how many active mass extractors are in the graph.
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

    /// Count how many active power generators are in the graph.
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

    /// Return the kinds of all units currently under construction or upgrade.
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

    /// Return the definitions for every active unit in the graph.
    pub fn active_unit_blueprints<'a>(&'a self) -> Vec<&'a UnitDef> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.is_active())
            .filter_map(|n| self.units.def(&n.unit_id))
            .collect()
    }

    /// Total build power of all active builders.
    pub fn total_active_build_power(&self) -> f64 {
        self.active_units()
            .iter()
            .map(|&b| builder_power(b, &self.graph, &self.units))
            .sum()
    }

    /// Derive the economy from all active units, applying adjacency bonuses
    /// to producers.
    pub fn derive_economy(&self) -> EconomyState {
        let active_nodes: Vec<NodeId> = self.active_units();

        let mut net_mass = MassRate::zero();
        let mut net_energy = EnergyRate::zero();
        let mut mass_storage_cap = Mass::zero();
        let mut energy_storage_cap = Energy::zero();

        for node_id in active_nodes {
            let kind = &self.graph[node_id].unit_id;
            let Some(def) = self.units.def(kind) else {
                continue;
            };

            let mut mass_income = def.mass_income();
            let mut energy_income = def.energy_income();

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
            mass_storage_cap = mass_storage_cap + Mass::from_raw(def.mass_storage());
            energy_storage_cap = energy_storage_cap + Energy::from_raw(def.energy_storage());
        }

        EconomyState {
            net_mass_income: net_mass,
            net_energy_income: net_energy,
            mass_storage: mass_storage_cap,
            energy_storage: energy_storage_cap,
            mass_storage_cap,
            energy_storage_cap,
        }
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
        self.graph
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
            .collect()
    }

    /// Validate that every builder in `builders` is idle and is a real builder.
    fn validate_builders(
        &self,
        builders: &[NodeId],
        target: &UnitKind,
    ) -> Result<(), GraphSimError> {
        if builders.is_empty() {
            return Err(GraphSimError::NoBuilders);
        }

        let busy = self.busy_builders();

        for &builder in builders {
            if busy.contains(&builder) {
                return Err(GraphSimError::BuilderBusy(builder));
            }
            if !is_builder_node(builder, &self.graph, &self.units) {
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
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, target)?;

        let cost = self
            .units
            .build_cost(target)
            .ok_or_else(|| GraphSimError::NotBuildable(target.clone()))?;

        let (started_by, assisted_by) = self.split_builders(builders, target);
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

    /// Split builders into those capable of building the target and assistants.
    fn split_builders(&self, builders: &[NodeId], target: &UnitKind) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut started_by = Vec::new();
        let mut assisted_by = Vec::new();
        for &builder in builders {
            let builder_unit_id = &self.graph[builder].unit_id;
            if self.units.can_build(builder_unit_id, target) {
                started_by.push(builder);
            } else {
                assisted_by.push(builder);
            }
        }
        (started_by, assisted_by)
    }

    /// Start an upgrade of `old_node` to `target` using the given idle builders.
    pub fn start_upgrade_project(
        &mut self,
        target: &UnitKind,
        old_node: NodeId,
        builders: &[NodeId],
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, target)?;

        let old_unit_id = self.graph[old_node].unit_id.clone();
        let recipe = self
            .units
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
    pub fn assist_project(
        &mut self,
        target_node: NodeId,
        builders: &[NodeId],
    ) -> Result<(), GraphSimError> {
        self.validate_builders(builders, &UnitKind::Commander)?;

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
    /// Returns the node ids of any units that completed during this tick. The
    /// caller is responsible for calling [`UnitGraph::derive_economy`] after
    /// completions if it wants income and storage caps to reflect the new units.
    pub fn tick(&mut self, economy: &mut EconomyState, dt: f64) -> Vec<NodeId> {
        if dt <= 0.0 {
            return Vec::new();
        }

        let active_projects = self.collect_active_projects();
        if active_projects.is_empty() {
            self.apply_idle_income(economy, dt);
            self.time += dt;
            return Vec::new();
        }

        let (project_powers, project_mass_drain, project_energy_drain) =
            self.compute_project_drain(&active_projects);

        let tick_result = apply_tick_graph(project_mass_drain, project_energy_drain, economy, dt);

        let completed_nodes = self.apply_project_progress(
            &active_projects,
            &project_powers,
            tick_result.effective_factor,
            dt,
        );

        economy.mass_storage = tick_result.new_mass_storage;
        economy.energy_storage = tick_result.new_energy_storage;
        self.time += dt;

        self.process_completed_nodes(&completed_nodes);

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
    fn compute_project_drain(&self, active_projects: &[NodeId]) -> (Vec<f64>, f64, f64) {
        let mut project_powers: Vec<f64> = Vec::with_capacity(active_projects.len());
        let mut total_mass_drain = 0.0;
        let mut total_energy_drain = 0.0;

        for &target_node in active_projects {
            let target_id = &self.graph[target_node].unit_id;
            let Some(cost) = self.units.build_cost(target_id) else {
                project_powers.push(0.0);
                continue;
            };

            let power: f64 = self
                .graph
                .graph
                .edges_directed(target_node.0, Direction::Incoming)
                .map(|edge| {
                    builder_power(NodeId::new(edge.source().index()), &self.graph, &self.units)
                })
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

    /// Handle side effects of completed nodes: adjacency and build events.
    fn process_completed_nodes(&mut self, completed_nodes: &[NodeId]) {
        self.apply_completion_adjacency(completed_nodes);
        self.emit_build_events(completed_nodes);
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
    fn emit_build_events(&mut self, completed_nodes: &[NodeId]) {
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
                unit_name: self.units.display_name(&unit_id),
                node_id: target_node,
            });
        }
    }

    /// Collect income for one tick with no active projects.
    fn apply_idle_income(&self, economy: &mut EconomyState, dt: f64) {
        let dt = Time::from_raw(dt);
        economy.mass_storage = (economy.mass_storage + economy.net_mass_income * dt)
            .min(economy.mass_storage_cap)
            .max(Mass::zero());
        economy.energy_storage = (economy.energy_storage + economy.net_energy_income * dt)
            .min(economy.energy_storage_cap)
            .max(Energy::zero());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tick::GameTick;
    use crate::engine::unit_command::{UnitAction, UnitCommand};
    use crate::units::{TechLevel, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn acu_starting_economy() {
        let units = load_units();
        let state = derive_economy(&units, &[UnitKind::Commander]);

        assert!((state.net_mass_income - crate::quantities::MassRate::from_raw(1.0)).abs() < 1e-9);
        assert!(
            (state.net_energy_income - crate::quantities::EnergyRate::from_raw(20.0)).abs() < 1e-9
        );
        assert!((state.mass_storage - crate::quantities::Mass::from_raw(650.0)).abs() < 1e-9);
        assert!((state.energy_storage - crate::quantities::Energy::from_raw(3900.0)).abs() < 1e-9);
    }

    #[test]
    fn derive_economy_subtracts_maintenance() {
        let units = load_units();
        let state = derive_economy(&units, &[UnitKind::Commander, UnitKind::Mex(TechLevel::T1)]);

        // ACU: +1 mass/s, +20 energy/s. T1 mex: +2 mass/s, -2 energy/s maintenance.
        assert!((state.net_mass_income - crate::quantities::MassRate::from_raw(3.0)).abs() < 1e-9);
        assert!(
            (state.net_energy_income - crate::quantities::EnergyRate::from_raw(18.0)).abs() < 1e-9
        );
    }

    #[test]
    fn unit_graph_builds_t1_mex() {
        let units = load_units();
        let mut graph = UnitGraph::new(&[UnitKind::Commander], units.clone());
        let mut economy = graph.derive_economy();

        graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        for _ in 0..60 {
            if !graph.tick(&mut economy, 1.0).is_empty() {
                break;
            }
        }

        assert!(
            graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1)),
            "expected a T1 mex completion event, got {:?}",
            graph.events
        );
    }

    #[test]
    fn forecast_build_project_drains_storage() {
        let units = load_units();
        let mut graph = UnitGraph::new(&[UnitKind::Commander], units.clone());
        let economy = graph.derive_economy();

        let initial_mass = economy.mass_storage.value();
        let initial_energy = economy.energy_storage.value();

        graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        let mut projected_graph = graph.clone();
        let mut projected_economy = economy;
        for _ in 0..5 {
            projected_graph.tick(&mut projected_economy, 1.0);
        }

        assert!(projected_economy.mass_storage.value() < initial_mass);
        assert!(projected_economy.energy_storage.value() < initial_energy);
    }

    #[test]
    fn forecast_contains_build_completion_event() {
        let units = load_units();
        let mut graph = UnitGraph::new(&[UnitKind::Commander], units.clone());
        let economy = graph.derive_economy();

        graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        let mut projected_graph = graph.clone();
        let mut projected_economy = economy;
        for _ in 0..60 {
            if !projected_graph.tick(&mut projected_economy, 1.0).is_empty() {
                break;
            }
        }

        assert!(
            projected_graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1)),
            "expected T1 mex completion in forecast, got {:?}",
            projected_graph.events
        );
    }

    #[test]
    fn forecast_does_not_mutate_original_graph() {
        let units = load_units();
        let mut graph = UnitGraph::new(&[UnitKind::Commander], units.clone());
        let economy = graph.derive_economy();

        graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        let initial_time = graph.time;
        let initial_mass = economy.mass_storage.value();

        let mut projected_graph = graph.clone();
        let mut projected_economy = economy.clone();
        for _ in 0..10 {
            projected_graph.tick(&mut projected_economy, 1.0);
        }

        assert!((graph.time - initial_time).abs() < f64::EPSILON);
        assert!((economy.mass_storage.value() - initial_mass).abs() < f64::EPSILON);
    }

    #[test]
    fn command_delay_is_applied_by_caller_tick_filter() {
        let units = load_units();
        let mut graph = UnitGraph::new(&[UnitKind::Commander], units.clone());
        let mut economy = graph.derive_economy();

        // A command issued at tick 0 that executes at tick 3.
        let cmd = UnitCommand::new(
            GameTick(3),
            UnitAction::Build {
                unit: UnitKind::Mex(TechLevel::T1),
                builders: vec![NodeId::new(0)],
            },
        );

        let mut tick = GameTick::FIRST;
        while tick.0 < 3 {
            if cmd.tick == tick {
                apply_command(&mut graph, &cmd).unwrap();
            }
            graph.tick(&mut economy, 1.0);
            tick = tick.next();
        }

        assert!(
            !graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1)),
            "mex should not complete before delayed start"
        );

        // Now execute the delayed command.
        apply_command(&mut graph, &cmd).unwrap();
        for _ in 0..60 {
            graph.tick(&mut economy, 1.0);
            if graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1))
            {
                break;
            }
        }

        assert!(
            graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1)),
            "mex should complete after delayed start"
        );
    }

    fn apply_command(graph: &mut UnitGraph, cmd: &UnitCommand) -> Result<(), GraphSimError> {
        match &cmd.action {
            UnitAction::Build { unit, builders } => {
                graph.start_project(unit, builders)?;
            }
            UnitAction::Assist { project, builders } => {
                graph.assist_project(*project, builders)?;
            }
            UnitAction::Upgrade {
                target,
                old_node,
                builders,
            } => {
                graph.start_upgrade_project(target, *old_node, builders)?;
            }
        }
        Ok(())
    }
}
