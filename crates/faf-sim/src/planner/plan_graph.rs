//! ACU-rooted plan graph for build-order visualisation.
//!
//! Unlike the backward-chained [`DependencyGraph`], the plan graph starts from
//! the ACU and expands forward through build and upgrade rules. It includes
//! both the technology chain (factories, engineers) and the economic
//! infrastructure (mass extractors, power generators) required to reach a
//! goal unit.
//!
//! Edges have two kinds:
//!
//! - `Build` — the source unit can construct the target unit.
//! - `Upgrade` — the source unit can be upgraded into the target unit.
//!
//! The graph is intentionally simplified: it shows one representative of each
//! common tier (T1/T2/T3 factories, engineers, mexes, pgens) plus the goal.
//! Faction-specific names are normalised to the abstract `UnitKind` labels.

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;

use crate::units::{TechLevel, UnitKind, Units};

/// Kind of action represented by an edge in the plan graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanEdgeKind {
    /// Source unit constructs the target unit.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
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
/// The returned graph contains:
///
/// - The ACU as the root.
/// - All common factory, engineer, mex and pgen tiers up to the technology
///   level required by the goal.
/// - The goal unit itself.
/// - `Build` edges for same-tier construction (e.g. T2 factory -> T2 engineer).
/// - `Upgrade` edges for tier progression (e.g. T1 factory -> T2 factory).
/// - The actual build edge from the goal's legal builder(s) to the goal.
pub fn build_plan_graph(units: &Units, goal: &UnitKind) -> Result<DiGraph<UnitKind, PlanEdgeKind>, PlanGraphError> {
    let max_tech = max_tech_needed(units, goal);
    let relevant = relevant_unit_kinds(max_tech, goal);

    let mut graph = DiGraph::<UnitKind, PlanEdgeKind>::new();
    let mut indices: HashMap<UnitKind, NodeIndex> = HashMap::new();
    for kind in &relevant {
        indices.insert(kind.clone(), graph.add_node(kind.clone()));
    }

    add_build_edges(units, &relevant, &indices, &mut graph);
    add_upgrade_edges(units, &relevant, &indices, &mut graph);

    let acu = UnitKind::Commander;
    if !is_reachable(&graph, &acu, goal) {
        return Err(PlanGraphError::GoalUnreachable(goal.clone()));
    }

    Ok(graph)
}

/// Determine the highest technology tier that must be achieved to build
/// `goal`.
///
/// This is the maximum tier appearing in the goal's prerequisite chain or
/// among its direct builders.
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
        UnitKind::Engineer(t)
        | UnitKind::Factory(t)
        | UnitKind::Mex(t)
        | UnitKind::Pgen(t) => Some(*t),
        UnitKind::Commander => Some(TechLevel::T1),
        UnitKind::Unique(_) => None,
    }
}

/// Collect the unit kinds that should appear in the plan graph.
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

    if !kinds.contains(goal) {
        kinds.push(goal.clone());
    }

    kinds
}

/// Add `Build` edges between relevant units.
///
/// We keep edges that represent natural same-tier construction, plus the
/// edge from the goal's actual builder(s) to the goal. Higher-tier builders
/// constructing lower-tier targets (e.g. a T3 engineer rebuilding a T1 mex)
/// are omitted to keep the plan graph readable.
fn add_build_edges(
    units: &Units,
    relevant: &[UnitKind],
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    let relevant_set: HashSet<&UnitKind> = relevant.iter().collect();

    for target in relevant {
        let Some(recipe) = units.build_recipe(target) else {
            continue;
        };

        for builder in &recipe.builder_options {
            if !relevant_set.contains(builder) {
                continue;
            }

            // Always show the actual builder edge for the goal.
            if target == relevant.last().expect("goal is last") {
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
    relevant: &[UnitKind],
    indices: &HashMap<UnitKind, NodeIndex>,
    graph: &mut DiGraph<UnitKind, PlanEdgeKind>,
) {
    let relevant_set: HashSet<&UnitKind> = relevant.iter().collect();

    for from in relevant {
        for recipe in units.upgrade_recipes(from) {
            if relevant_set.contains(&recipe.from) && relevant_set.contains(&recipe.to) {
                add_edge(graph, indices, &recipe.from, &recipe.to, PlanEdgeKind::Upgrade);
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
        _ => false,
    }
}

/// True if `goal` is reachable from `start` in `graph`.
fn is_reachable(graph: &DiGraph<UnitKind, PlanEdgeKind>, start: &UnitKind, goal: &UnitKind) -> bool {
    let mut bfs = Bfs::new(
        graph,
        graph
            .node_indices()
            .find(|i| graph[*i] == *start)
            .expect("start node exists"),
    );
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
        let graph = build_plan_graph(&units, &goal).expect("fatboy should be reachable");

        let node_set: HashSet<UnitKind> = graph.raw_nodes().iter().map(|n| n.weight.clone()).collect();

        assert!(node_set.contains(&UnitKind::Commander));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T2)));
        assert!(node_set.contains(&UnitKind::Factory(TechLevel::T3)));
        assert!(node_set.contains(&UnitKind::Engineer(TechLevel::T3)));
        assert!(node_set.contains(&UnitKind::Mex(TechLevel::T2)));
        assert!(node_set.contains(&UnitKind::Pgen(TechLevel::T2)));
        assert!(node_set.contains(&goal));
    }

    #[test]
    fn fatboy_plan_graph_has_build_and_upgrade_edges() {
        let units = load_units();
        let goal = UnitKind::Unique(crate::units::UnitId("UEL0401".to_string()));
        let graph = build_plan_graph(&units, &goal).expect("fatboy should be reachable");

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
    }

    #[test]
    fn t1_pgen_plan_graph_stops_at_t1() {
        let units = load_units();
        let goal = UnitKind::Pgen(TechLevel::T1);
        let graph = build_plan_graph(&units, &goal).expect("t1 pgen should be reachable");

        let node_set: HashSet<UnitKind> = graph.raw_nodes().iter().map(|n| n.weight.clone()).collect();

        assert!(node_set.contains(&UnitKind::Commander));
        assert!(node_set.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(!node_set.contains(&UnitKind::Factory(TechLevel::T2)));
    }
}
