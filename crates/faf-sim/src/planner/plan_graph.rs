//! ACU-rooted plan graph for build-order planning and training.
//!
//! The plan graph starts from the ACU and expands forward through build and
//! upgrade rules. It contains the fixed technology chain (factories, engineers)
//! and economic infrastructure (mass extractors, power generators) needed to
//! reach any supported goal. The concrete goal unit is abstracted away: a
//! synthetic [`Goal`] node is attached to the T3 engineer and represents any
//! expensive target described only by tech level and cost.
//!
//! Edges have two kinds:
//!
//! - `Build` — the source unit can construct the target unit (or goal).
//! - `Upgrade` — the source unit can be upgraded into the target unit.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::planner::core::Goal;
use crate::units::{TechLevel, UnitKind, Units};

/// Kind of action represented by an edge in the plan graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanEdgeKind {
    /// Source unit constructs the target unit or goal.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
}

/// Strategic focus of a plan-graph edge.
///
/// This is used by the hierarchical policy network: the direction head picks
/// a focus, and the action head then scores only the legal edges that belong
/// to that focus.
///
/// Factory upgrades are *not* a direction. They are handled by a dedicated
/// `upgrade_head` because teching up is a separate strategic decision from
/// choosing an economic focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCategory {
    /// Edges that increase mass income.
    IncreaseMass,
    /// Edges that increase energy income or storage.
    IncreaseEnergy,
    /// Edges that increase build power.
    IncreaseBP,
    /// The single edge that builds the abstract goal.
    Goal,
}

impl EdgeCategory {
    /// All possible strategic directions.
    pub const ALL: [EdgeCategory; 4] = [
        EdgeCategory::IncreaseMass,
        EdgeCategory::IncreaseEnergy,
        EdgeCategory::IncreaseBP,
        EdgeCategory::Goal,
    ];

    /// Categorise an edge based on what the target node contributes.
    pub fn categorize(target: &PlanNode) -> EdgeCategory {
        match target {
            PlanNode::Goal(_) => EdgeCategory::Goal,
            PlanNode::Unit(UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex) => {
                EdgeCategory::IncreaseMass
            }
            PlanNode::Unit(UnitKind::Pgen(_) | UnitKind::EnergyStorage) => {
                EdgeCategory::IncreaseEnergy
            }
            PlanNode::Unit(UnitKind::Engineer(_) | UnitKind::Factory(_) | UnitKind::Commander) => {
                EdgeCategory::IncreaseBP
            }
            PlanNode::Unit(UnitKind::Unique(_)) => EdgeCategory::IncreaseBP,
        }
    }
}

/// A node in the plan graph: either a concrete unit or the abstract goal.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanNode {
    /// A concrete unit kind from the fixed tech/economy tree.
    Unit(UnitKind),
    /// The abstract target being planned or trained for.
    Goal(Goal),
}

impl PlanNode {
    /// If this node is a unit, return its kind.
    pub fn as_unit(&self) -> Option<&UnitKind> {
        match self {
            PlanNode::Unit(kind) => Some(kind),
            PlanNode::Goal(_) => None,
        }
    }

    /// If this node is the goal, return it.
    pub fn as_goal(&self) -> Option<&Goal> {
        match self {
            PlanNode::Goal(goal) => Some(goal),
            PlanNode::Unit(_) => None,
        }
    }
}

/// A simplified, ACU-rooted plan graph for a goal.
#[derive(Debug, Clone)]
pub struct PlanGraph {
    plan: DiGraph<PlanNode, PlanEdgeKind>,
    goal: Goal,
}

impl PlanGraph {
    /// Wrap an existing graph and its goal.
    pub fn new(plan: DiGraph<PlanNode, PlanEdgeKind>, goal: Goal) -> Self {
        Self { plan, goal }
    }

    /// The goal this plan graph was built for.
    pub fn goal(&self) -> &Goal {
        &self.goal
    }

    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<PlanNode, PlanEdgeKind> {
        &self.plan
    }
}

/// A universal, faction-abstract plan graph containing the fixed tech/economy
/// tree. A concrete [`PlanGraph`] for any goal is derived by attaching a
/// [`Goal`] node to the T3 engineer.
#[derive(Debug, Clone, Default)]
pub struct UniversalPlanGraph {
    graph: DiGraph<UnitKind, PlanEdgeKind>,
}

impl UniversalPlanGraph {
    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<UnitKind, PlanEdgeKind> {
        &self.graph
    }

    /// Return a plan graph for `goal` by attaching a synthetic goal node to the
    /// T3 engineer in the fixed tech/economy tree.
    pub fn with_goal(&self, goal: Goal) -> PlanGraph {
        let mut graph = DiGraph::<PlanNode, PlanEdgeKind>::new();
        let mut indices: HashMap<UnitKind, NodeIndex> = HashMap::new();

        for node in self.graph.node_indices() {
            let kind = self.graph[node].clone();
            indices.insert(kind.clone(), graph.add_node(PlanNode::Unit(kind)));
        }

        for edge in self.graph.edge_references() {
            let s = indices[&self.graph[edge.source()]];
            let t = indices[&self.graph[edge.target()]];
            graph.add_edge(s, t, *edge.weight());
        }

        let t3_eng = UnitKind::Engineer(TechLevel::T3);
        let &t3_eng_idx = indices
            .get(&t3_eng)
            .expect("fixed plan graph must contain T3 engineer");
        let goal_idx = graph.add_node(PlanNode::Goal(goal));
        graph.add_edge(t3_eng_idx, goal_idx, PlanEdgeKind::Build);

        PlanGraph::new(graph, goal)
    }
}

/// Build a simplified, ACU-rooted plan graph for `goal`.
///
/// This is a convenience wrapper around [`build_universal_plan_graph`] followed
/// by [`UniversalPlanGraph::with_goal`].
pub fn build_plan_graph(units: &Units, goal: Goal) -> PlanGraph {
    units.universal_plan_graph().with_goal(goal)
}

/// Build the universal plan graph containing the fixed tech/economy tree.
pub fn build_universal_plan_graph(_units: &Units) -> UniversalPlanGraph {
    let mut graph = DiGraph::<UnitKind, PlanEdgeKind>::new();
    let mut indices: HashMap<UnitKind, NodeIndex> = HashMap::new();

    let kinds = common_unit_kinds();
    for kind in &kinds {
        indices.insert(kind.clone(), graph.add_node(kind.clone()));
    }

    add_natural_build_edges(&indices, &mut graph);
    add_upgrade_edges(&indices, &mut graph);

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

/// Add the fixed set of natural build edges to the tech/economy tree.
fn add_natural_build_edges(
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    let edges: Vec<(UnitKind, UnitKind)> = vec![
        // ACU bootstraps T1 infrastructure.
        (UnitKind::Commander, UnitKind::Factory(TechLevel::T1)),
        (UnitKind::Commander, UnitKind::Mex(TechLevel::T1)),
        (UnitKind::Commander, UnitKind::Pgen(TechLevel::T1)),
        // Factories build same-tier engineers.
        (
            UnitKind::Factory(TechLevel::T1),
            UnitKind::Engineer(TechLevel::T1),
        ),
        (
            UnitKind::Factory(TechLevel::T2),
            UnitKind::Engineer(TechLevel::T2),
        ),
        (
            UnitKind::Factory(TechLevel::T3),
            UnitKind::Engineer(TechLevel::T3),
        ),
        // Engineers build same-tier economy.
        (
            UnitKind::Engineer(TechLevel::T1),
            UnitKind::Mex(TechLevel::T1),
        ),
        (
            UnitKind::Engineer(TechLevel::T1),
            UnitKind::Pgen(TechLevel::T1),
        ),
        (
            UnitKind::Engineer(TechLevel::T2),
            UnitKind::Mex(TechLevel::T2),
        ),
        (
            UnitKind::Engineer(TechLevel::T2),
            UnitKind::Pgen(TechLevel::T2),
        ),
        (
            UnitKind::Engineer(TechLevel::T3),
            UnitKind::Mex(TechLevel::T3),
        ),
        (
            UnitKind::Engineer(TechLevel::T3),
            UnitKind::Pgen(TechLevel::T3),
        ),
        // Any engineer can build energy storage.
        (UnitKind::Engineer(TechLevel::T1), UnitKind::EnergyStorage),
        (UnitKind::Engineer(TechLevel::T2), UnitKind::EnergyStorage),
        (UnitKind::Engineer(TechLevel::T3), UnitKind::EnergyStorage),
    ];

    for (from, to) in edges {
        let &from_idx = indices.get(&from).expect("from node exists");
        let &to_idx = indices.get(&to).expect("to node exists");
        if graph.find_edge(from_idx, to_idx).is_none() {
            graph.add_edge(from_idx, to_idx, PlanEdgeKind::Build);
        }
    }
}

/// Add the fixed set of upgrade edges to the tech/economy tree.
fn add_upgrade_edges(
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    let edges: Vec<(UnitKind, UnitKind)> = vec![
        (
            UnitKind::Factory(TechLevel::T1),
            UnitKind::Factory(TechLevel::T2),
        ),
        (
            UnitKind::Factory(TechLevel::T2),
            UnitKind::Factory(TechLevel::T3),
        ),
        (UnitKind::Mex(TechLevel::T1), UnitKind::Mex(TechLevel::T2)),
        (UnitKind::Mex(TechLevel::T2), UnitKind::Mex(TechLevel::T3)),
        (UnitKind::Mex(TechLevel::T2), UnitKind::CapT2Mex),
        (UnitKind::Mex(TechLevel::T3), UnitKind::CapT3Mex),
        (UnitKind::Pgen(TechLevel::T1), UnitKind::Pgen(TechLevel::T2)),
        (UnitKind::Pgen(TechLevel::T2), UnitKind::Pgen(TechLevel::T3)),
    ];

    for (from, to) in edges {
        let &from_idx = indices.get(&from).expect("from node exists");
        let &to_idx = indices.get(&to).expect("to node exists");
        if graph.find_edge(from_idx, to_idx).is_none() {
            graph.add_edge(from_idx, to_idx, PlanEdgeKind::Upgrade);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    fn fatboy_goal() -> Goal {
        Goal {
            tech_level: TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        }
    }

    #[test]
    fn plan_graph_includes_tech_and_economy() {
        let units = load_units();
        let plan_graph = build_plan_graph(&units, fatboy_goal());
        let graph = plan_graph.graph();

        let contains_unit = |kind: &UnitKind| {
            graph
                .node_indices()
                .any(|i| graph[i].as_unit() == Some(kind))
        };

        assert!(contains_unit(&UnitKind::Commander));
        assert!(contains_unit(&UnitKind::Factory(TechLevel::T1)));
        assert!(contains_unit(&UnitKind::Factory(TechLevel::T2)));
        assert!(contains_unit(&UnitKind::Factory(TechLevel::T3)));
        assert!(contains_unit(&UnitKind::Engineer(TechLevel::T3)));
        assert!(contains_unit(&UnitKind::Mex(TechLevel::T2)));
        assert!(contains_unit(&UnitKind::Pgen(TechLevel::T2)));
        assert!(contains_unit(&UnitKind::CapT2Mex));
        assert!(contains_unit(&UnitKind::CapT3Mex));
        assert!(contains_unit(&UnitKind::EnergyStorage));
    }

    #[test]
    fn plan_graph_has_build_and_upgrade_edges() {
        let units = load_units();
        let plan_graph = build_plan_graph(&units, fatboy_goal());
        let graph = plan_graph.graph();

        let contains_edge = |from: &UnitKind, to: &UnitKind, kind: PlanEdgeKind| {
            graph.edge_indices().any(|e| {
                let edge = graph.edge_endpoints(e).unwrap();
                graph[edge.0].as_unit() == Some(from)
                    && graph[edge.1].as_unit() == Some(to)
                    && graph[e] == kind
            })
        };

        assert!(contains_edge(
            &UnitKind::Factory(TechLevel::T1),
            &UnitKind::Engineer(TechLevel::T1),
            PlanEdgeKind::Build
        ));
        assert!(contains_edge(
            &UnitKind::Factory(TechLevel::T1),
            &UnitKind::Factory(TechLevel::T2),
            PlanEdgeKind::Upgrade
        ));
        assert!(contains_edge(
            &UnitKind::Mex(TechLevel::T2),
            &UnitKind::CapT2Mex,
            PlanEdgeKind::Upgrade
        ));
        assert!(contains_edge(
            &UnitKind::Engineer(TechLevel::T1),
            &UnitKind::EnergyStorage,
            PlanEdgeKind::Build
        ));
    }

    #[test]
    fn plan_graph_has_goal_edge_from_t3_engineer() {
        let units = load_units();
        let goal = fatboy_goal();
        let plan_graph = build_plan_graph(&units, goal);
        let graph = plan_graph.graph();

        let goal_edges: Vec<_> = graph
            .edge_indices()
            .filter(|&e| {
                let (s, t) = graph.edge_endpoints(e).unwrap();
                graph[s].as_unit() == Some(&UnitKind::Engineer(TechLevel::T3))
                    && graph[t].as_goal() == Some(&goal)
            })
            .collect();

        assert_eq!(
            goal_edges.len(),
            1,
            "T3 engineer should have exactly one goal edge"
        );
    }

    #[test]
    fn fixed_graph_size_is_constant() {
        let units = load_units();
        let g1 = build_plan_graph(&units, fatboy_goal());
        let g2 = build_plan_graph(
            &units,
            Goal {
                tech_level: TechLevel::T3,
                mass_cost: 1.0,
                energy_cost: 1.0,
                build_time: 1.0,
            },
        );
        assert_eq!(
            g1.graph().edge_count(),
            g2.graph().edge_count(),
            "edge count should be independent of goal cost"
        );
    }
}
