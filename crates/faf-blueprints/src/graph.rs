//! Symbolic build/upgrade/cap graph derived from a [`BlueprintLibrary`].
//!
//! This graph is intended for visualization, planning, and scheduling. It is
//! backed by [`petgraph::graph::DiGraph`], with nodes representing unit kinds
//! and directed edges representing the relationships that make construction,
//! upgrade, or capping possible:
//!
//! - **BuiltBy** — a builder unit can construct the target when an optional
//!   prerequisite unit already exists (`builder -> target`).
//! - **UpgradesInto** — an existing unit can be transformed into the next tier
//!   (`from -> to`).
//! - **CapsInto** — an existing unit can be capped (e.g., a mex surrounded by
//!   mass storages) (`from -> to`).
//!
//! Costs are intentionally not part of the graph; they are resolved from the
//! runtime economic table in [`BlueprintLibrary`](super::BlueprintLibrary).

use std::collections::HashMap;

use petgraph::graph::{DiGraph, EdgeReference, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::types::{UnitCategory, UnitKind, UnitRole};

/// A node in the blueprint graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlueprintNode {
    /// Canonical identity of the unit.
    pub kind: UnitKind,
    /// Human-readable name.
    pub display_name: String,
    /// Functional role.
    pub role: UnitRole,
    /// UI category.
    pub category: UnitCategory,
}

/// The kind of relationship a directed edge represents in the blueprint graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlueprintEdge {
    /// The source unit can build the target unit.
    ///
    /// `prereq` is the unit that must already exist before construction can
    /// start, if any.
    BuiltBy {
        /// Unit that must already be finished before construction starts.
        prereq: Option<UnitKind>,
    },
    /// The source unit can be upgraded into the target unit.
    ///
    /// The source unit's own build power drives the upgrade, so the edge does
    /// not carry a separate builder list.
    UpgradesInto,
    /// The source unit can be capped into the target unit.
    ///
    /// Capping is a separate transformation from tier upgrades (e.g., a T2 mex
    /// surrounded by mass storages).
    CapsInto,
}

/// The complete symbolic build/upgrade graph.
#[derive(Debug, Clone)]
pub struct BlueprintGraph {
    /// The underlying petgraph directed graph.
    pub graph: DiGraph<BlueprintNode, BlueprintEdge>,
    /// Fast lookup from a unit's canonical identity to its node index.
    kind_to_index: HashMap<UnitKind, NodeIndex>,
}

impl PartialEq for BlueprintGraph {
    fn eq(&self, other: &Self) -> bool {
        if self.graph.node_count() != other.graph.node_count()
            || self.graph.edge_count() != other.graph.edge_count()
        {
            return false;
        }
        if !self
            .graph
            .node_weights()
            .zip(other.graph.node_weights())
            .all(|(a, b)| a == b)
        {
            return false;
        }
        fn sorted_edges(
            g: &DiGraph<BlueprintNode, BlueprintEdge>,
        ) -> Vec<(usize, usize, &BlueprintEdge)> {
            let mut v: Vec<_> = g
                .edge_references()
                .map(|e| (e.source().index(), e.target().index(), e.weight()))
                .collect();
            v.sort_by_key(|e| (e.0, e.1));
            v
        }
        let a = sorted_edges(&self.graph);
        let b = sorted_edges(&other.graph);
        a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.0 == y.0 && x.1 == y.1 && x.2 == y.2)
    }
}

impl Eq for BlueprintGraph {}

#[derive(Serialize)]
struct SerializedGraph<'a> {
    nodes: &'a [BlueprintNode],
    edges: Vec<SerializedEdge<'a>>,
}

#[derive(Serialize)]
struct SerializedEdge<'a> {
    source: usize,
    target: usize,
    #[serde(flatten)]
    weight: &'a BlueprintEdge,
}

impl Serialize for BlueprintGraph {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let nodes: Vec<_> = self.graph.node_weights().cloned().collect();
        let edges: Vec<_> = self
            .graph
            .edge_references()
            .map(|e| SerializedEdge {
                source: e.source().index(),
                target: e.target().index(),
                weight: e.weight(),
            })
            .collect();
        SerializedGraph {
            nodes: &nodes,
            edges,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
struct DeserializedGraph {
    nodes: Vec<BlueprintNode>,
    edges: Vec<DeserializedEdge>,
}

#[derive(Deserialize)]
struct DeserializedEdge {
    source: usize,
    target: usize,
    #[serde(flatten)]
    weight: BlueprintEdge,
}

impl<'de> Deserialize<'de> for BlueprintGraph {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = DeserializedGraph::deserialize(deserializer)?;
        let mut graph = Self::new();
        for node in raw.nodes {
            graph.add_node(node);
        }
        for edge in raw.edges {
            let from = NodeIndex::new(edge.source);
            let to = NodeIndex::new(edge.target);
            graph.add_edge(from, to, edge.weight);
        }
        Ok(graph)
    }
}

impl BlueprintGraph {
    /// Create an empty blueprint graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            kind_to_index: HashMap::new(),
        }
    }

    /// Add a node to the graph and remember its kind-based lookup entry.
    pub fn add_node(&mut self, node: BlueprintNode) -> NodeIndex {
        let idx = self.graph.add_node(node);
        // SAFETY: we just added the node, so the weight exists.
        let kind = self.graph[idx].kind.clone();
        self.kind_to_index.insert(kind, idx);
        idx
    }

    /// Add a directed edge between two existing nodes.
    pub fn add_edge(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        edge: BlueprintEdge,
    ) -> Option<petgraph::graph::EdgeIndex> {
        if self.graph.node_weight(from).is_some() && self.graph.node_weight(to).is_some() {
            Some(self.graph.add_edge(from, to, edge))
        } else {
            None
        }
    }

    /// Find a node by kind.
    pub fn node(&self, kind: &UnitKind) -> Option<&BlueprintNode> {
        self.kind_to_index.get(kind).map(|&idx| &self.graph[idx])
    }

    /// Find the node index for a given unit kind.
    pub fn node_index(&self, kind: &UnitKind) -> Option<NodeIndex> {
        self.kind_to_index.get(kind).copied()
    }

    /// All `BuiltBy` edges that target the given kind.
    ///
    /// Yields `(builder_node_index, edge_weight)` for every incoming build edge
    /// to `target`.
    pub fn builds_for<'a>(
        &'a self,
        target: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(target).copied() {
            Some(target_idx) => self
                .graph
                .edges_directed(target_idx, Direction::Incoming)
                .filter(|e| matches!(e.weight(), BlueprintEdge::BuiltBy { .. }))
                .map(move |e| (e.source(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// All `BuiltBy` edges where the given kind acts as a builder.
    ///
    /// Yields `(target_node_index, edge_weight)` for every outgoing build edge
    /// from `builder`.
    pub fn builds_by<'a>(
        &'a self,
        builder: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(builder).copied() {
            Some(builder_idx) => self
                .graph
                .edges_directed(builder_idx, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), BlueprintEdge::BuiltBy { .. }))
                .map(move |e| (e.target(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// All upgrade edges that start from the given kind.
    ///
    /// Yields `(target_node_index, edge_weight)` for every outgoing
    /// `UpgradesInto` edge from `from`.
    pub fn upgrades_from<'a>(
        &'a self,
        from: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(from).copied() {
            Some(from_idx) => self
                .graph
                .edges_directed(from_idx, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), BlueprintEdge::UpgradesInto))
                .map(move |e| (e.target(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// All upgrade edges that end at the given kind.
    ///
    /// Yields `(source_node_index, edge_weight)` for every incoming
    /// `UpgradesInto` edge to `to`.
    pub fn upgrades_to<'a>(
        &'a self,
        to: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(to).copied() {
            Some(to_idx) => self
                .graph
                .edges_directed(to_idx, Direction::Incoming)
                .filter(|e| matches!(e.weight(), BlueprintEdge::UpgradesInto))
                .map(move |e| (e.source(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// All cap edges that start from the given kind.
    ///
    /// Yields `(target_node_index, edge_weight)` for every outgoing
    /// `CapsInto` edge from `from`.
    pub fn caps_from<'a>(
        &'a self,
        from: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(from).copied() {
            Some(from_idx) => self
                .graph
                .edges_directed(from_idx, Direction::Outgoing)
                .filter(|e| matches!(e.weight(), BlueprintEdge::CapsInto))
                .map(move |e| (e.target(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// All cap edges that end at the given kind.
    ///
    /// Yields `(source_node_index, edge_weight)` for every incoming
    /// `CapsInto` edge to `to`.
    pub fn caps_to<'a>(
        &'a self,
        to: &UnitKind,
    ) -> impl Iterator<Item = (NodeIndex, &'a BlueprintEdge)> + 'a {
        match self.kind_to_index.get(to).copied() {
            Some(to_idx) => self
                .graph
                .edges_directed(to_idx, Direction::Incoming)
                .filter(|e| matches!(e.weight(), BlueprintEdge::CapsInto))
                .map(move |e| (e.source(), e.weight()))
                .collect::<Vec<_>>()
                .into_iter(),
            None => Vec::new().into_iter(),
        }
    }

    /// Iterate over all edges in the graph.
    pub fn edge_references(&self) -> impl Iterator<Item = EdgeReference<'_, BlueprintEdge>> {
        self.graph.edge_references()
    }
}

impl Default for BlueprintGraph {
    fn default() -> Self {
        Self::new()
    }
}
