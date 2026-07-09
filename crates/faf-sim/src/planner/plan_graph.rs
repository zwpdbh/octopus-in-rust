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

use crate::engine::{NodeId, Simulation};
use crate::planner::core::Goal;
use crate::units::{TechLevel, UnitKind, Units};

/// Concrete action an edge represents in the plan graph.
///
/// This describes *how* the action is executed: either a builder constructs a
/// new unit/goal, or an existing unit is upgraded in-place. It is orthogonal to
/// [`EdgeCategory`], which describes the *strategic focus* of the edge (mass,
/// energy, build power, or goal).
///
/// For example, a factory-upgrade edge has `EdgeAction::Upgrade` and
/// `EdgeCategory::UpgradeTech`, while a factory building an engineer has
/// `EdgeAction::Build` and `EdgeCategory::IncreaseBP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeAction {
    /// Source unit constructs the target unit or goal.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
}

/// Strategic focus of a plan-graph edge.
///
/// This is now also the output space of the direction-only policy network.
/// The network picks one of these high-level directions, and a heuristic layer
/// turns it into a concrete `SimAction`.
///
/// This is orthogonal to [`EdgeAction`]. `EdgeAction` describes *how* an edge
/// is executed (build vs upgrade); `EdgeCategory` describes *what strategic
/// bucket* the edge falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCategory {
    /// Edges that increase mass income.
    IncreaseMass,
    /// Edges that increase energy income.
    IncreaseEnergy,
    /// Edges that increase build power.
    IncreaseBP,
    /// Edges that build energy storage.
    IncreaseEnergyStorage,
    /// The single edge that builds the abstract goal.
    Goal,
    /// Edges that upgrade factory tech.
    UpgradeTech,
}

impl EdgeCategory {
    /// All possible strategic directions.
    pub const ALL: [EdgeCategory; 6] = [
        EdgeCategory::IncreaseMass,
        EdgeCategory::IncreaseEnergy,
        EdgeCategory::IncreaseBP,
        EdgeCategory::IncreaseEnergyStorage,
        EdgeCategory::Goal,
        EdgeCategory::UpgradeTech,
    ];

    /// Categorise an edge based on its action and what the target node contributes.
    ///
    /// Upgrade edges are split out separately: factory upgrades are the
    /// [`EdgeCategory::UpgradeTech`] direction, while mex and power-generator
    /// upgrades fall under [`EdgeCategory::IncreaseMass`] and
    /// [`EdgeCategory::IncreaseEnergy`] respectively.
    pub fn categorize(action: EdgeAction, target: &PlanNode) -> EdgeCategory {
        match action {
            EdgeAction::Upgrade => match target {
                PlanNode::Unit(UnitKind::Factory(_)) => EdgeCategory::UpgradeTech,
                PlanNode::Unit(UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex) => {
                    EdgeCategory::IncreaseMass
                }
                PlanNode::Unit(UnitKind::Pgen(_)) => EdgeCategory::IncreaseEnergy,
                _ => EdgeCategory::IncreaseBP,
            },
            EdgeAction::Build => match target {
                PlanNode::Goal(_) => EdgeCategory::Goal,
                PlanNode::Unit(UnitKind::EnergyStorage) => EdgeCategory::IncreaseEnergyStorage,
                PlanNode::Unit(UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex) => {
                    EdgeCategory::IncreaseMass
                }
                PlanNode::Unit(UnitKind::Pgen(_)) => EdgeCategory::IncreaseEnergy,
                PlanNode::Unit(
                    UnitKind::Engineer(_) | UnitKind::Factory(_) | UnitKind::Commander,
                ) => EdgeCategory::IncreaseBP,
                PlanNode::Unit(UnitKind::Unique(_)) => EdgeCategory::IncreaseBP,
            },
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
    plan: DiGraph<PlanNode, EdgeAction>,
    goal: Goal,
}

impl PlanGraph {
    /// Wrap an existing graph and its goal.
    pub fn new(plan: DiGraph<PlanNode, EdgeAction>, goal: Goal) -> Self {
        Self { plan, goal }
    }

    /// The goal this plan graph was built for.
    pub fn goal(&self) -> &Goal {
        &self.goal
    }

    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<PlanNode, EdgeAction> {
        &self.plan
    }
}

/// A universal, faction-abstract plan graph containing the fixed tech/economy
/// tree. A concrete [`PlanGraph`] for any goal is derived by attaching a
/// [`Goal`] node to the T3 engineer.
#[derive(Debug, Clone, Default)]
pub struct UniversalPlanGraph {
    graph: DiGraph<UnitKind, EdgeAction>,
}

impl UniversalPlanGraph {
    /// Borrow the underlying petgraph structure.
    pub fn graph(&self) -> &DiGraph<UnitKind, EdgeAction> {
        &self.graph
    }

    /// Return a plan graph for `goal` by attaching a synthetic goal node to the
    /// T3 engineer in the fixed tech/economy tree.
    pub fn with_goal(&self, goal: Goal) -> PlanGraph {
        let mut graph = DiGraph::<PlanNode, EdgeAction>::new();
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
        graph.add_edge(t3_eng_idx, goal_idx, EdgeAction::Build);

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
    let mut graph = DiGraph::<UnitKind, EdgeAction>::new();
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
    graph: &mut DiGraph<UnitKind, EdgeAction>,
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
            graph.add_edge(from_idx, to_idx, EdgeAction::Build);
        }
    }
}

/// Add the fixed set of upgrade edges to the tech/economy tree.
fn add_upgrade_edges(
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, EdgeAction>,
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
            graph.add_edge(from_idx, to_idx, EdgeAction::Upgrade);
        }
    }
}

/// True if `edge` can be executed in `state`.
///
/// The source node must be a unit that is owned and active. For build edges the
/// target must not already be owned or under construction, a capable idle
/// builder must exist, and the mex cap must not be exceeded. For upgrade edges
/// [`can_upgrade`] must hold.
pub fn is_plan_edge_legal(
    action: EdgeAction,
    source: &PlanNode,
    target: &PlanNode,
    state: &Simulation,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> bool {
    let Some(source_kind) = source.as_unit() else {
        return false;
    };

    if !state.graph.has_completed_unit(source_kind) {
        return false;
    }

    match action {
        EdgeAction::Build => {
            let can_build = match target.as_goal() {
                Some(goal) => {
                    !state.graph.goal_reached(goal)
                        && !state.graph.goal_project_active()
                        && can_build_goal(source_kind, goal)
                }
                None => {
                    let target_kind = target.as_unit().expect("build target must be unit or goal");
                    !state.graph.has_completed_unit(target_kind)
                        && !state.graph.active_target_unit_ids().contains(target_kind)
                        && !would_exceed_mex_cap(state, config, target_kind)
                }
            };
            can_build && is_idle_builder(state, units, source_kind)
        }
        EdgeAction::Upgrade => {
            let source_kind = source.as_unit().expect("upgrade source must be a unit");
            let target_kind = target.as_unit().expect("upgrade target must be a unit");
            can_upgrade(state, units, source_kind, target_kind)
        }
    }
}

/// True if building `target_kind` would exceed the configured mex cap.
fn would_exceed_mex_cap(
    state: &Simulation,
    config: &crate::planner::core::PlannerConfig,
    target_kind: &UnitKind,
) -> bool {
    if !is_mex_kind(target_kind) {
        return false;
    }
    state.graph.count_active_mex() >= config.max_mex_count
}

/// True if `kind` is a mass extractor (any tech level or capped variant).
fn is_mex_kind(kind: &UnitKind) -> bool {
    matches!(
        kind,
        UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex
    )
}

/// True if `builder` is allowed to start the abstract `goal` project.
///
/// For now all goals are built by a T3 engineer.
fn can_build_goal(builder: &UnitKind, _goal: &Goal) -> bool {
    matches!(builder, UnitKind::Engineer(TechLevel::T3))
}

/// True if the state has an active, idle builder of the given kind.
pub fn is_idle_builder(state: &Simulation, _units: &Units, kind: &UnitKind) -> bool {
    state
        .graph
        .idle_builders()
        .iter()
        .any(|&id| state.graph[id].unit_id == *kind)
}

/// True if `source` can be upgraded into `target` now.
///
/// The source unit must be active and not already busy, and there must be an
/// idle builder capable of performing the upgrade.
pub fn can_upgrade(
    state: &Simulation,
    units: &Units,
    source: &UnitKind,
    target: &UnitKind,
) -> bool {
    // Find an active source unit that is not already upgrading or building.
    let source_nodes: Vec<_> = state
        .graph
        .graph
        .graph
        .node_weights()
        .filter(|n| n.is_active() && n.unit_id == *source)
        .map(|n| n.id)
        .collect();

    if source_nodes.is_empty() {
        return false;
    }

    // Find an idle builder that can perform this upgrade.
    let recipe = units
        .upgrade_recipes(source)
        .iter()
        .find(|r| r.to == *target);

    let Some(recipe) = recipe else {
        return false;
    };

    recipe.builder_options.iter().any(|builder_kind| {
        state
            .graph
            .idle_builders()
            .iter()
            .any(|&id| state.graph[id].unit_id == *builder_kind)
    })
}

/// Find an active source node of the given kind for an upgrade edge.
pub fn find_upgrade_source(state: &Simulation, source_kind: &UnitKind) -> Option<NodeId> {
    state
        .graph
        .graph
        .graph
        .node_weights()
        .find(|n| n.is_active() && n.unit_id == *source_kind)
        .map(|n| n.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::PlannerConfig;

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

        let contains_edge = |from: &UnitKind, to: &UnitKind, kind: EdgeAction| {
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
            EdgeAction::Build
        ));
        assert!(contains_edge(
            &UnitKind::Factory(TechLevel::T1),
            &UnitKind::Factory(TechLevel::T2),
            EdgeAction::Upgrade
        ));
        assert!(contains_edge(
            &UnitKind::Mex(TechLevel::T2),
            &UnitKind::CapT2Mex,
            EdgeAction::Upgrade
        ));
        assert!(contains_edge(
            &UnitKind::Engineer(TechLevel::T1),
            &UnitKind::EnergyStorage,
            EdgeAction::Build
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
    fn mex_build_blocked_when_cap_reached() {
        let units = load_units();
        let goal = fatboy_goal();
        let plan_graph = build_plan_graph(&units, goal);
        let graph = plan_graph.graph();

        // Find the ACU -> T1 mex build edge.
        let mex_edge = graph
            .edge_indices()
            .find(|&e| {
                let (s, t) = graph.edge_endpoints(e).unwrap();
                graph[s].as_unit() == Some(&UnitKind::Commander)
                    && graph[t].as_unit() == Some(&UnitKind::Mex(TechLevel::T1))
                    && graph[e] == EdgeAction::Build
            })
            .expect("plan graph should have ACU -> T1 mex build edge");

        let (source_idx, target_idx) = graph.edge_endpoints(mex_edge).unwrap();
        let source = &graph[source_idx];
        let target = &graph[target_idx];

        // With zero active mexes and a cap of zero, building is illegal.
        let state = Simulation::new(&[UnitKind::Commander], units.clone(), 10);
        let zero_cap_config = PlannerConfig {
            max_mex_count: 0,
            ..PlannerConfig::default()
        };
        assert!(
            !is_plan_edge_legal(
                EdgeAction::Build,
                source,
                target,
                &state,
                &units,
                &zero_cap_config
            ),
            "new mex build should be illegal when cap is already reached"
        );

        // With a cap of one, building the first mex is legal.
        let one_cap_config = PlannerConfig {
            max_mex_count: 1,
            ..PlannerConfig::default()
        };
        assert!(
            is_plan_edge_legal(
                EdgeAction::Build,
                source,
                target,
                &state,
                &units,
                &one_cap_config
            ),
            "first mex build should be legal when below the cap"
        );
    }

    #[test]
    fn mex_upgrade_remains_legal_at_cap() {
        let units = load_units();
        let goal = fatboy_goal();
        let plan_graph = build_plan_graph(&units, goal);
        let graph = plan_graph.graph();

        // Find the T2 mex -> CapT2Mex upgrade edge.
        let cap_edge = graph
            .edge_indices()
            .find(|&e| {
                let (s, t) = graph.edge_endpoints(e).unwrap();
                graph[s].as_unit() == Some(&UnitKind::Mex(TechLevel::T2))
                    && graph[t].as_unit() == Some(&UnitKind::CapT2Mex)
                    && graph[e] == EdgeAction::Upgrade
            })
            .expect("plan graph should have T2 mex -> CapT2Mex upgrade edge");

        let (source_idx, target_idx) = graph.edge_endpoints(cap_edge).unwrap();
        let source = &graph[source_idx];
        let target = &graph[target_idx];

        // One active T2 mex and a cap of one: upgrading should still be legal
        // because upgrades do not increase the total mex count.
        let state = Simulation::new(
            &[
                UnitKind::Commander,
                UnitKind::Factory(TechLevel::T1),
                UnitKind::Engineer(TechLevel::T2),
                UnitKind::Mex(TechLevel::T2),
            ],
            units.clone(),
            10,
        );
        let cap_config = PlannerConfig {
            max_mex_count: 1,
            ..PlannerConfig::default()
        };
        assert!(
            is_plan_edge_legal(
                EdgeAction::Upgrade,
                source,
                target,
                &state,
                &units,
                &cap_config
            ),
            "mex upgrade should remain legal even when the cap is reached"
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
