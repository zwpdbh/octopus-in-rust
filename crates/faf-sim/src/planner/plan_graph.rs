//! ACU-rooted plan graph for build-order visualisation.
//!
//! The plan graph starts from the ACU and expands forward through build and
//! upgrade rules. It includes the technology chain (factories, engineers) and
//! the economic infrastructure (mass extractors, power generators) required to
//! reach any supported goal unit.
//!
//! The graph is intentionally made *universal*: one graph contains all common
//! units and all candidate goal units. A concrete goal is just a pointer into
//! this graph; `PlanGraph::for_goal` produces a goal-specific view by pruning
//! everything that is not an ancestor of the chosen goal.
//!
//! Edges have two kinds:
//!
//! - `Build` — the source unit can construct the target unit.
//! - `Upgrade` — the source unit can be upgraded into the target unit.

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Bfs, EdgeRef};

use crate::units::{TechLevel, UnitKind, Units};

/// Kind of action represented by an edge in the plan graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanEdgeKind {
    /// Source unit constructs the target unit.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
}

/// Strategic focus of a plan-graph edge.
///
/// This is used by the hierarchical policy network: the direction head picks
/// a focus, and the action head then scores only the legal edges that belong
/// to that focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCategory {
    /// Edges that increase mass income.
    Mass,
    /// Edges that increase energy income or storage.
    Energy,
    /// Edges that increase build power.
    BuildPower,
    /// Edges that directly advance toward the concrete goal unit.
    Progress,
}

impl EdgeCategory {
    /// All possible strategic directions.
    pub const ALL: [EdgeCategory; 4] = [
        EdgeCategory::Mass,
        EdgeCategory::Energy,
        EdgeCategory::BuildPower,
        EdgeCategory::Progress,
    ];

    /// Categorise an edge based on what the target unit contributes.
    pub fn categorize(_source: &UnitKind, target: &UnitKind) -> EdgeCategory {
        match target {
            UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex => EdgeCategory::Mass,
            UnitKind::Pgen(_) | UnitKind::EnergyStorage => EdgeCategory::Energy,
            UnitKind::Engineer(_) | UnitKind::Factory(_) => EdgeCategory::BuildPower,
            UnitKind::Commander | UnitKind::Unique(_) => EdgeCategory::Progress,
        }
    }
}

/// A simplified, ACU-rooted plan graph for a goal unit.
///
/// This is a thin wrapper around the internal [`DiGraph`] so that domain-specific
/// methods (distance heuristics, relevance queries, etc.) can be added later
/// without leaking the raw graph type into every consumer.
#[derive(Debug, Clone)]
pub struct PlanGraph {
    plan: DiGraph<UnitKind, PlanEdgeKind>,
    goal: UnitKind,
}

impl PlanGraph {
    /// Wrap an existing graph and its goal.
    pub fn new(plan: DiGraph<UnitKind, PlanEdgeKind>, goal: UnitKind) -> Self {
        Self { plan, goal }
    }

    /// The goal unit this plan graph was built for.
    pub fn goal(&self) -> &UnitKind {
        &self.goal
    }

    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<UnitKind, PlanEdgeKind> {
        &self.plan
    }
}

/// A universal, faction-abstract plan graph containing all relevant units.
///
/// Candidate goal units (T4 experimentals and selected important T3 structures)
/// live alongside the common technology and economic tree. A concrete
/// [`PlanGraph`] view for any supported goal can be derived on demand.
#[derive(Debug, Clone, Default)]
pub struct UniversalPlanGraph {
    graph: DiGraph<UnitKind, PlanEdgeKind>,
}

impl UniversalPlanGraph {
    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<UnitKind, PlanEdgeKind> {
        &self.graph
    }

    /// Return a goal-specific view containing the goal and all common units up
    /// to the technology tier it requires.
    ///
    /// This keeps the same economic/tech breadth as the old per-goal builder:
    /// reaching a T3/Experimental goal includes every T1-T3 common unit, while
    /// a T1 goal includes only the T1 common units.
    pub fn for_goal(&self, units: &Units, goal: &UnitKind) -> Result<PlanGraph, PlanGraphError> {
        let max_tech = max_tech_needed(units, goal);
        let relevant: HashSet<UnitKind> = relevant_unit_kinds(max_tech, goal).into_iter().collect();

        let mut subgraph = DiGraph::<UnitKind, PlanEdgeKind>::new();
        let mut new_indices: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        for old_idx in self.graph.node_indices() {
            let kind = &self.graph[old_idx];
            if relevant.contains(kind) {
                new_indices.insert(old_idx, subgraph.add_node(kind.clone()));
            }
        }

        for edge in self.graph.edge_references() {
            if relevant.contains(&self.graph[edge.source()])
                && relevant.contains(&self.graph[edge.target()])
            {
                let s = new_indices[&edge.source()];
                let t = new_indices[&edge.target()];
                if subgraph.find_edge(s, t).is_none() {
                    subgraph.add_edge(s, t, *edge.weight());
                }
            }
        }

        if !is_reachable(&subgraph, &UnitKind::Commander, goal) {
            return Err(PlanGraphError::GoalUnreachable(goal.clone()));
        }

        Ok(PlanGraph::new(subgraph, goal.clone()))
    }
}

/// Error returned when a plan graph cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanGraphError {
    /// The goal cannot be reached from the ACU using the known rules.
    GoalUnreachable(UnitKind),
}

impl std::fmt::Display for PlanGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanGraphError::GoalUnreachable(kind) => {
                write!(f, "goal {:?} is not reachable from the ACU", kind)
            }
        }
    }
}

impl std::error::Error for PlanGraphError {}

/// Build a simplified, ACU-rooted plan graph for `goal`.
///
/// This is a convenience wrapper around [`build_universal_plan_graph`] followed
/// by [`UniversalPlanGraph::for_goal`].
pub fn build_plan_graph(units: &Units, goal: &UnitKind) -> Result<PlanGraph, PlanGraphError> {
    units.universal_plan_graph().for_goal(units, goal)
}

/// Determine the highest technology tier that must be achieved to build
/// `goal`.
fn max_tech_needed(units: &Units, goal: &UnitKind) -> TechLevel {
    let mut max = TechLevel::T1;

    for kind in units.prerequisite_chain(goal) {
        if let Some(t) = tech_level(&kind) {
            max = max.max(t);
        }
    }

    for builder in units.builders_for(goal) {
        if let Some(t) = tech_level(&builder) {
            max = max.max(t);
        }
    }

    max
}

/// Return the technology tier of a common unit kind, if it has one.
fn tech_level(kind: &UnitKind) -> Option<TechLevel> {
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => {
            Some(*t)
        }
        UnitKind::Commander => Some(TechLevel::T1),
        UnitKind::CapT2Mex => Some(TechLevel::T2),
        UnitKind::CapT3Mex => Some(TechLevel::T3),
        UnitKind::EnergyStorage => Some(TechLevel::T1),
        UnitKind::Unique(_) => None,
    }
}

/// Collect the unit kinds that should appear in a goal-specific plan graph.
fn relevant_unit_kinds(max_tech: TechLevel, goal: &UnitKind) -> Vec<UnitKind> {
    let mut kinds = Vec::new();
    kinds.push(UnitKind::Commander);

    for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
        if tech > max_tech {
            break;
        }
        kinds.push(UnitKind::Factory(tech));
        kinds.push(UnitKind::Engineer(tech));
        kinds.push(UnitKind::Mex(tech));
        kinds.push(UnitKind::Pgen(tech));
    }

    // Capped mexes and energy storage are available once engineers exist.
    if max_tech >= TechLevel::T2 {
        kinds.push(UnitKind::CapT2Mex);
    }
    if max_tech >= TechLevel::T3 {
        kinds.push(UnitKind::CapT3Mex);
    }
    kinds.push(UnitKind::EnergyStorage);

    if !kinds.contains(goal) {
        kinds.push(goal.clone());
    }

    kinds
}

/// Build the universal plan graph containing all common units and candidate
/// goal units.
pub fn build_universal_plan_graph(units: &Units) -> UniversalPlanGraph {
    let mut kinds = common_unit_kinds();
    for goal in units.goal_candidates() {
        if !kinds.contains(goal) {
            kinds.push(goal.clone());
        }
    }

    let mut graph = DiGraph::<UnitKind, PlanEdgeKind>::new();
    let mut indices: HashMap<UnitKind, NodeIndex> = HashMap::new();
    for kind in &kinds {
        indices.insert(kind.clone(), graph.add_node(kind.clone()));
    }
    let kind_set: HashSet<&UnitKind> = kinds.iter().collect();

    add_build_edges(units, &kinds, &kind_set, &indices, &mut graph);
    add_upgrade_edges(units, &kinds, &kind_set, &indices, &mut graph);

    UniversalPlanGraph { graph }
}

/// Return the common unit kinds that are always part of the plan graph.
fn common_unit_kinds() -> Vec<UnitKind> {
    let mut kinds = vec![UnitKind::Commander];
    for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
        kinds.push(UnitKind::Factory(tech));
        kinds.push(UnitKind::Engineer(tech));
        kinds.push(UnitKind::Mex(tech));
        kinds.push(UnitKind::Pgen(tech));
    }
    kinds.push(UnitKind::CapT2Mex);
    kinds.push(UnitKind::CapT3Mex);
    kinds.push(UnitKind::EnergyStorage);
    kinds
}

/// Add `Build` edges between relevant units.
///
/// We keep natural same-tier construction edges for common units and the real
/// builder edges for every unique goal candidate.
fn add_build_edges(
    units: &Units,
    kinds: &[UnitKind],
    kind_set: &HashSet<&UnitKind>,
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    for target in kinds {
        let Some(recipe) = units.build_recipe(target) else {
            continue;
        };

        for builder in &recipe.builder_options {
            if !kind_set.contains(builder) {
                continue;
            }

            // Always show the actual builder edge for unique goal candidates.
            if matches!(target, UnitKind::Unique(_)) {
                add_edge(graph, indices, builder, target, PlanEdgeKind::Build);
                continue;
            }

            // Keep same-tier construction edges and ACU -> T1 infrastructure.
            if is_natural_build_edge(builder, target) {
                add_edge(graph, indices, builder, target, PlanEdgeKind::Build);
            }
        }
    }
}

/// Add `Upgrade` edges between relevant units.
fn add_upgrade_edges(
    units: &Units,
    kinds: &[UnitKind],
    kind_set: &HashSet<&UnitKind>,
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    for from in kinds {
        for recipe in units.upgrade_recipes(from) {
            if kind_set.contains(&recipe.from) && kind_set.contains(&recipe.to) {
                add_edge(
                    graph,
                    indices,
                    &recipe.from,
                    &recipe.to,
                    PlanEdgeKind::Upgrade,
                );
            }
        }
    }
}

/// Helper to insert an edge if it does not already exist.
fn add_edge(
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
    indices: &HashMap<UnitKind, NodeIndex>,
    from: &UnitKind,
    to: &UnitKind,
    kind: PlanEdgeKind,
) {
    let &from_idx = indices.get(from).expect("from node exists");
    let &to_idx = indices.get(to).expect("to node exists");
    if graph.find_edge(from_idx, to_idx).is_none() {
        graph.add_edge(from_idx, to_idx, kind);
    }
}

/// Decide whether a build edge is part of the natural progression tree.
///
/// Natural edges are:
/// - ACU constructing T1 infrastructure (factory, mex, pgen).
/// - A factory constructing an engineer of the same tier.
/// - An engineer constructing a mex or pgen of the same tier.
/// - Any engineer constructing energy storage.
///
/// This deliberately excludes higher-tier builders constructing lower-tier
/// structures or engineers constructing factories, both of which would create
/// cycles in the simplified plan graph.
fn is_natural_build_edge(builder: &UnitKind, target: &UnitKind) -> bool {
    use UnitKind;
    match (builder, target) {
        (UnitKind::Commander, UnitKind::Factory(TechLevel::T1)) => true,
        (UnitKind::Commander, UnitKind::Mex(TechLevel::T1)) => true,
        (UnitKind::Commander, UnitKind::Pgen(TechLevel::T1)) => true,
        (UnitKind::Factory(t1), UnitKind::Engineer(t2)) if t1 == t2 => true,
        (UnitKind::Engineer(t1), UnitKind::Mex(t2)) if t1 == t2 => true,
        (UnitKind::Engineer(t1), UnitKind::Pgen(t2)) if t1 == t2 => true,
        (UnitKind::Engineer(_), UnitKind::EnergyStorage) => true,
        _ => false,
    }
}

/// True if `goal` is reachable from `start` in `graph`.
fn is_reachable(
    graph: &DiGraph<UnitKind, PlanEdgeKind>,
    start: &UnitKind,
    goal: &UnitKind,
) -> bool {
    let Some(start_idx) = graph.node_indices().find(|i| graph[*i] == *start) else {
        return false;
    };
    let mut bfs = Bfs::new(graph, start_idx);
    while let Some(idx) = bfs.next(graph) {
        if graph[idx] == *goal {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn fatboy_plan_graph_includes_tech_and_economy() {
        let units = load_units();
        let goal = UnitKind::Unique(crate::units::UnitId("UEL0401".to_string()));
        let plan_graph = build_plan_graph(&units, &goal).expect("fatboy should be reachable");
        let graph = plan_graph.graph();

        let node_set: HashSet<UnitKind> =
            graph.raw_nodes().iter().map(|n| n.weight.clone()).collect();

        assert!(node_set.contains(&UnitKind::Commander));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T2)));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T3)));
        assert!(node_set.contains(&UnitKind::Engineer(TechLevel::T3)));
        assert!(node_set.contains(&UnitKind::Mex(TechLevel::T2)));
        assert!(node_set.contains(&UnitKind::Pgen(TechLevel::T2)));
        assert!(node_set.contains(&UnitKind::CapT2Mex));
        assert!(node_set.contains(&UnitKind::CapT3Mex));
        assert!(node_set.contains(&UnitKind::EnergyStorage));
        assert!(node_set.contains(&goal));
    }

    #[test]
    fn fatboy_plan_graph_has_build_and_upgrade_edges() {
        let units = load_units();
        let goal = UnitKind::Unique(crate::units::UnitId("UEL0401".to_string()));
        let plan_graph = build_plan_graph(&units, &goal).expect("fatboy should be reachable");
        let graph = plan_graph.graph();

        let edges: Vec<(UnitKind, UnitKind, PlanEdgeKind)> = graph
            .raw_edges()
            .iter()
            .map(|e| {
                (
                    graph[e.source()].clone(),
                    graph[e.target()].clone(),
                    e.weight,
                )
            })
            .collect();

        assert!(edges.contains(&(
            UnitKind::Factory(TechLevel::T1),
            UnitKind::Engineer(TechLevel::T1),
            PlanEdgeKind::Build
        )));
        assert!(edges.contains(&(
            UnitKind::Factory(TechLevel::T1),
            UnitKind::Factory(TechLevel::T2),
            PlanEdgeKind::Upgrade
        )));
        assert!(edges.contains(&(
            UnitKind::Engineer(TechLevel::T3),
            goal.clone(),
            PlanEdgeKind::Build
        )));
        assert!(edges.contains(&(
            UnitKind::Mex(TechLevel::T2),
            UnitKind::CapT2Mex,
            PlanEdgeKind::Upgrade
        )));
        assert!(edges.contains(&(
            UnitKind::Engineer(TechLevel::T1),
            UnitKind::EnergyStorage,
            PlanEdgeKind::Build
        )));
    }

    #[test]
    fn t1_pgen_plan_graph_stops_at_t1() {
        let units = load_units();
        let goal = UnitKind::Pgen(TechLevel::T1);
        let plan_graph = build_plan_graph(&units, &goal).expect("t1 pgen should be reachable");
        let graph = plan_graph.graph();

        let node_set: HashSet<UnitKind> =
            graph.raw_nodes().iter().map(|n| n.weight.clone()).collect();

        assert!(node_set.contains(&UnitKind::Commander));
        assert!(node_set.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(!node_set.contains(&UnitKind::Factory(TechLevel::T2)));
    }

    #[test]
    fn universal_graph_contains_multiple_goal_candidates() {
        let units = load_units();
        let universal = units.universal_plan_graph();
        let node_set: HashSet<UnitKind> = universal
            .graph
            .raw_nodes()
            .iter()
            .map(|n| n.weight.clone())
            .collect();

        assert!(node_set.contains(&UnitKind::Commander));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T3)));
        assert!(node_set.contains(&UnitKind::Mex(TechLevel::T3)));
        // Fatboy and Monkeylord are both T4 experimentals and should coexist.
        assert!(node_set.contains(&UnitKind::Unique(crate::units::UnitId(
            "UEL0401".to_string()
        ))));
        assert!(node_set.contains(&UnitKind::Unique(crate::units::UnitId(
            "URL0402".to_string()
        ))));
    }

    #[test]
    fn goal_views_are_independent_subgraphs() {
        let units = load_units();
        let fatboy = UnitKind::Unique(crate::units::UnitId("UEL0401".to_string()));
        let monkeylord = UnitKind::Unique(crate::units::UnitId("URL0402".to_string()));

        let fatboy_graph = build_plan_graph(&units, &fatboy).expect("fatboy reachable");
        let monkeylord_graph = build_plan_graph(&units, &monkeylord).expect("monkeylord reachable");

        let fatboy_nodes: HashSet<UnitKind> = fatboy_graph
            .graph()
            .raw_nodes()
            .iter()
            .map(|n| n.weight.clone())
            .collect();
        let monkeylord_nodes: HashSet<UnitKind> = monkeylord_graph
            .graph()
            .raw_nodes()
            .iter()
            .map(|n| n.weight.clone())
            .collect();

        // The common tech/eco tree is shared.
        assert!(fatboy_nodes.contains(&UnitKind::Factory(TechLevel::T3)));
        assert!(monkeylord_nodes.contains(&UnitKind::Factory(TechLevel::T3)));
        // Each view keeps only its own goal unit.
        assert!(fatboy_nodes.contains(&fatboy));
        assert!(!fatboy_nodes.contains(&monkeylord));
        assert!(monkeylord_nodes.contains(&monkeylord));
        assert!(!monkeylord_nodes.contains(&fatboy));
    }
}
