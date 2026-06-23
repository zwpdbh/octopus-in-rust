//! Graph-based build simulator.
//!
//! This simulator implements the graph-growth model from
//! `tutorials/my_model.md`: nodes are built units, edges record builder
//! assignments, and builders are indivisible (one target at a time). Multiple
//! projects may run concurrently as long as they use disjoint builder sets.

use faf_units::{DataIndex, Unit};

use crate::economy::{apply_tick_graph, compute_drain, RequestedBuildPower};
use crate::sim::{derive_economy, BuildEvent};
use crate::tech_graph::TechGraph;

/// Opaque identifier for a node in the build graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// One built unit in the growing build graph.
#[derive(Debug, Clone)]
pub struct UnitNode {
    /// Stable node identifier.
    pub id: NodeId,
    /// Blueprint id of the unit represented by this node.
    pub unit_id: String,
    /// When construction of this unit began.
    pub start_time: f64,
    /// When construction of this unit completed. `NaN` until finished.
    pub finish_time: f64,
}

/// The growing directed graph of built units and builder assignments.
#[derive(Debug, Clone, Default)]
pub struct BuildGraph {
    /// All unit nodes, ordered by creation time.
    pub nodes: Vec<UnitNode>,
    /// Directed edges `builder -> built unit`.
    pub edges: Vec<(NodeId, NodeId)>,
}

/// A project currently under construction.
#[derive(Debug, Clone)]
pub struct GraphProject {
    /// Node id of the unit being built.
    pub target_node: NodeId,
    /// Builder nodes assigned to this project. Builders are indivisible.
    pub builders: Vec<NodeId>,
    /// Remaining work in blueprint `BuildTime` units.
    pub remaining_work: f64,
    /// Time when this project started.
    pub start_time: f64,
}

/// Mutable simulation state for the graph model.
#[derive(Debug, Clone)]
pub struct GraphState {
    /// Current simulation time in seconds.
    pub time: f64,
    /// The build graph.
    pub graph: BuildGraph,
    /// Current economy state.
    pub economy: crate::economy::EconomyState,
    /// Builder nodes that are currently idle and available for new work.
    pub idle_builders: Vec<NodeId>,
    /// Projects currently under construction.
    pub active_projects: Vec<GraphProject>,
    /// Completed build events in chronological order.
    pub events: Vec<BuildEvent>,
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
            GraphSimError::BuilderBusy(id) => write!(f, "builder {} is busy", id.0),
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

/// True if `unit` is a real builder (commander, engineer, or factory) and has
/// a positive build rate.
fn is_builder(unit: &Unit) -> bool {
    (unit.has_category("COMMANDER")
        || unit.has_category("ENGINEER")
        || unit.has_category("FACTORY"))
        && unit.builder_capability().is_some()
}

/// True if the node represents a builder unit.
fn is_builder_node(node_id: NodeId, graph: &BuildGraph, index: &DataIndex) -> bool {
    let unit_id = &graph.nodes[node_id.0].unit_id;
    index
        .find_unit(unit_id)
        .map_or(false, |unit| is_builder(unit))
}

/// Build power contributed by a single builder node.
pub(crate) fn builder_power(node_id: NodeId, graph: &BuildGraph, index: &DataIndex) -> f64 {
    let unit_id = &graph.nodes[node_id.0].unit_id;
    let Some(unit) = index.find_unit(unit_id) else {
        return 0.0;
    };
    unit.builder_capability()
        .map(|cap| cap.build_rate)
        .unwrap_or(0.0)
}

impl GraphState {
    /// Create a new simulation state from the given starting units.
    ///
    /// All starting units are treated as already completed at time 0. Any
    /// builders among them are added to `idle_builders`.
    pub fn new(starting_units: &[&Unit]) -> Self {
        let mut graph = BuildGraph::default();
        let mut idle_builders = Vec::new();

        for (i, unit) in starting_units.iter().enumerate() {
            let node_id = NodeId(i);
            graph.nodes.push(UnitNode {
                id: node_id,
                unit_id: unit.id.clone(),
                start_time: 0.0,
                finish_time: 0.0,
            });
            if is_builder(unit) {
                idle_builders.push(node_id);
            }
        }

        let economy = derive_economy(starting_units);

        Self {
            time: 0.0,
            graph,
            economy,
            idle_builders,
            active_projects: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Return the ids of all completed (built) units.
    pub fn completed_units(&self) -> Vec<NodeId> {
        self.graph
            .nodes
            .iter()
            .filter(|n| !n.finish_time.is_nan())
            .map(|n| n.id)
            .collect()
    }

    /// True if the unit represented by `node_id` has been completed.
    pub fn is_completed(&self, node_id: NodeId) -> bool {
        self.graph
            .nodes
            .get(node_id.0)
            .map_or(false, |n| !n.finish_time.is_nan())
    }

    /// True if `builder` is currently idle.
    fn is_idle(&self, builder: NodeId) -> bool {
        self.idle_builders.contains(&builder)
    }

    /// Validate that every builder in `builders` is idle and is a real builder.
    fn validate_builders(
        &self,
        builders: &[NodeId],
        index: &DataIndex,
    ) -> Result<(), GraphSimError> {
        if builders.is_empty() {
            return Err(GraphSimError::NoBuilders);
        }
        for &builder in builders {
            if !self.is_idle(builder) {
                return Err(GraphSimError::BuilderBusy(builder));
            }
            if !is_builder_node(builder, &self.graph, index) {
                return Err(GraphSimError::CannotBuild {
                    builder: self.graph.nodes[builder.0].unit_id.clone(),
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
        target: &Unit,
        builders: &[NodeId],
        graph: &TechGraph,
    ) -> Result<NodeId, GraphSimError> {
        self.validate_builders(builders, graph.index())?;

        let stats = target
            .build_target_stats()
            .ok_or_else(|| GraphSimError::NotBuildable(target.id.clone()))?;

        // At least one builder must be able to build the target.
        let has_capable_builder = builders.iter().any(|&b| {
            let builder_unit_id = &self.graph.nodes[b.0].unit_id;
            graph
                .can_build(builder_unit_id, &target.id)
                .unwrap_or(false)
        });
        if !has_capable_builder {
            return Err(GraphSimError::CannotBuild {
                builder: builders
                    .first()
                    .map(|b| self.graph.nodes[b.0].unit_id.clone())
                    .unwrap_or_default(),
                target: target.id.clone(),
            });
        }

        let node_id = NodeId(self.graph.nodes.len());
        self.graph.nodes.push(UnitNode {
            id: node_id,
            unit_id: target.id.clone(),
            start_time: self.time,
            finish_time: f64::NAN,
        });

        for &builder in builders {
            self.graph.edges.push((builder, node_id));
        }

        self.active_projects.push(GraphProject {
            target_node: node_id,
            builders: builders.to_vec(),
            remaining_work: stats.build_time,
            start_time: self.time,
        });

        self.idle_builders.retain(|&b| !builders.contains(&b));
        Ok(node_id)
    }

    /// Assign additional idle `builders` to an already active project.
    ///
    /// Assisting builders do not need to be capable of building the target;
    /// they only need to be real builders.
    pub fn assist_project(
        &mut self,
        project_index: usize,
        builders: &[NodeId],
        _graph: &TechGraph,
    ) -> Result<(), GraphSimError> {
        self.validate_builders(builders, _graph.index())?;

        let project = self
            .active_projects
            .get(project_index)
            .ok_or(GraphSimError::ProjectNotFound)?;

        for &builder in builders {
            self.graph.edges.push((builder, project.target_node));
        }

        self.active_projects[project_index]
            .builders
            .extend(builders.iter().copied());
        self.idle_builders.retain(|&b| !builders.contains(&b));
        Ok(())
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Returns the node ids of any units that completed during this tick.
    pub fn tick(&mut self, index: &DataIndex, dt: f64) -> Vec<NodeId> {
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
            let target_id = &self.graph.nodes[project.target_node.0].unit_id;
            let Some(target) = index.find_unit(target_id) else {
                project_powers.push(0.0);
                continue;
            };
            let Some(stats) = target.build_target_stats() else {
                project_powers.push(0.0);
                continue;
            };
            let power: f64 = project
                .builders
                .iter()
                .map(|&b| builder_power(b, &self.graph, index))
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
                self.graph.nodes[project.target_node.0].finish_time = finish_time;
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
            let unit_id = self.graph.nodes[project.target_node.0].unit_id.clone();

            // Return builders to the idle pool.
            for &builder in &project.builders {
                self.idle_builders.push(builder);
            }

            // The completed unit itself becomes available as a builder.
            if let Some(unit) = index.find_unit(&unit_id) {
                if is_builder(unit) {
                    self.idle_builders.push(project.target_node);
                }
            }

            self.events.push(BuildEvent {
                time: self.graph.nodes[project.target_node.0].finish_time,
                unit_id: unit_id.clone(),
                unit_name: index
                    .find_unit(&unit_id)
                    .map(|u| u.display_name())
                    .unwrap_or_else(|| unit_id.clone()),
            });
        }

        // Re-derive economy from all completed units.
        if !completed_nodes.is_empty() {
            let owned_units: Vec<&Unit> = self
                .graph
                .nodes
                .iter()
                .filter(|n| !n.finish_time.is_nan())
                .filter_map(|n| index.find_unit(&n.unit_id))
                .collect();
            self.economy = derive_economy(&owned_units);
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
    use crate::tech_graph::TechGraph;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn acu_builds_t1_pgen() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");

        let mut state = GraphState::new(&[acu]);
        let acu_node = NodeId(0);
        state
            .start_project(pgen, &[acu_node], &graph)
            .expect("ACU can build T1 pgen");

        let acu_rate = acu.builder_capability().unwrap().build_rate;
        let expected_ticks = (pgen.build_target_stats().unwrap().build_time / acu_rate).ceil();
        let mut completed = Vec::new();
        for _ in 0..(expected_ticks as usize + 5) {
            completed.extend(state.tick(&index, 1.0));
            if !completed.is_empty() {
                break;
            }
        }

        assert_eq!(completed.len(), 1);
        assert_eq!(state.graph.nodes[completed[0].0].unit_id, "URB1101");
        assert!(state.time > 0.0);
        assert!(state.idle_builders.contains(&acu_node));
    }

    #[test]
    fn builders_are_indivisible() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");
        let mex = index.find_unit("URB1103").expect("T1 mex exists");

        let mut state = GraphState::new(&[acu]);
        let acu_node = NodeId(0);
        state
            .start_project(pgen, &[acu_node], &graph)
            .expect("ACU can build pgen");

        // ACU is busy, so starting another project with it must fail.
        let result = state.start_project(mex, &[acu_node], &graph);
        assert!(
            matches!(result, Err(GraphSimError::BuilderBusy(id)) if id == acu_node),
            "ACU should be busy"
        );
    }

    #[test]
    fn concurrent_projects_with_disjoint_builders() {
        let index = load_index();
        let graph = TechGraph::new(&index);
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let factory = index.find_unit("URB0101").expect("T1 factory exists");
        let eng = index.find_unit("URL0105").expect("T1 engineer exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");

        let mut state = GraphState::new(&[acu]);
        let acu_node = NodeId(0);
        let factory_node = state
            .start_project(factory, &[acu_node], &graph)
            .expect("ACU builds factory");

        // Tick until the factory completes.
        for _ in 0..1000 {
            state.tick(&index, 1.0);
            if state.is_completed(factory_node) {
                break;
            }
        }
        assert!(state.is_completed(factory_node), "factory should finish");

        // Start two concurrent projects: factory builds an engineer, ACU builds
        // a pgen. Both use disjoint builder sets.
        let eng_node = state
            .start_project(eng, &[factory_node], &graph)
            .expect("factory builds engineer");
        let pgen_node = state
            .start_project(pgen, &[acu_node], &graph)
            .expect("ACU builds pgen");

        assert_eq!(state.active_projects.len(), 2);
        assert!(
            state.idle_builders.is_empty(),
            "all builders should be assigned"
        );

        // Both should make progress each tick.
        let before0 = state.active_projects[0].remaining_work;
        let before1 = state.active_projects[1].remaining_work;
        state.tick(&index, 1.0);
        assert!(state.active_projects[0].remaining_work < before0);
        assert!(state.active_projects[1].remaining_work < before1);

        // Finish both.
        for _ in 0..1000 {
            state.tick(&index, 1.0);
            if state.is_completed(eng_node) && state.is_completed(pgen_node) {
                break;
            }
        }
        assert!(state.is_completed(eng_node), "engineer should finish");
        assert!(state.is_completed(pgen_node), "pgen should finish");
    }

    #[test]
    fn energy_stall_reduces_mass_income() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");

        // Force an energy-stalled project by starting a huge drain with no
        // energy income. We do this by creating a fake project state manually.
        let mut state = GraphState::new(&[acu]);
        state.economy.net_mass_income = 10.0;
        state.economy.net_energy_income = 0.0;
        state.economy.energy_storage = 0.0;
        state.economy.mass_storage = 0.0;

        let result = apply_tick_graph(0.0, 100.0, &state.economy, 1.0);
        assert!(result.energy_stalled);
        assert_eq!(result.scaled_net_mass_income, 0.0);
    }
}
