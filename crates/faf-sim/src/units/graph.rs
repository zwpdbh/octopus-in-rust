//! Symbolic build/upgrade graph derived from a [`BlueprintLibrary`].
//!
//! This graph is intended for visualization, planning, and scheduling. It has
//! two edge types:
//!
//! - **Build edges** — a target unit can be freshly constructed when its
//!   prerequisite exists and one of its builders is available.
//! - **Upgrade edges** — an existing unit can be transformed into another unit.
//!
//! Costs are intentionally not part of the graph; they are resolved from the
//! runtime economic table in [`BlueprintLibrary`](super::BlueprintLibrary).

use super::types::{UnitCategory, UnitKind, UnitRole};

/// A node in the blueprint graph.
#[derive(Debug, Clone, PartialEq)]
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

/// A build edge: `prereq` must exist, then any of `builders` can create `target`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildEdge {
    /// Unit being constructed.
    pub target: UnitKind,
    /// Unit that must already be finished before construction starts.
    pub prereq: Option<UnitKind>,
    /// Legal builders for the construction.
    pub builders: Vec<UnitKind>,
}

/// An upgrade edge: `from` can be upgraded into `to` by any of `builders`.
#[derive(Debug, Clone, PartialEq)]
pub struct UpgradeEdge {
    /// Source unit.
    pub from: UnitKind,
    /// Destination unit.
    pub to: UnitKind,
    /// Legal builders for the upgrade.
    pub builders: Vec<UnitKind>,
}

/// The complete symbolic build/upgrade graph.
#[derive(Debug, Clone, PartialEq)]
pub struct BlueprintGraph {
    pub nodes: Vec<BlueprintNode>,
    pub build_edges: Vec<BuildEdge>,
    pub upgrade_edges: Vec<UpgradeEdge>,
}

impl BlueprintGraph {
    /// Find a node by kind.
    pub fn node(&self, kind: &UnitKind) -> Option<&BlueprintNode> {
        self.nodes.iter().find(|n| &n.kind == kind)
    }

    /// All build edges whose target is the given kind.
    pub fn builds_for<'a>(&'a self, target: &UnitKind) -> impl Iterator<Item = &'a BuildEdge> + 'a {
        let target = target.clone();
        self.build_edges.iter().filter(move |e| e.target == target)
    }

    /// All build edges that a given builder can participate in.
    pub fn builds_by<'a>(&'a self, builder: &UnitKind) -> impl Iterator<Item = &'a BuildEdge> + 'a {
        let builder = builder.clone();
        self.build_edges
            .iter()
            .filter(move |e| e.builders.contains(&builder))
    }

    /// All upgrade edges that start from the given kind.
    pub fn upgrades_from<'a>(
        &'a self,
        from: &UnitKind,
    ) -> impl Iterator<Item = &'a UpgradeEdge> + 'a {
        let from = from.clone();
        self.upgrade_edges.iter().filter(move |e| e.from == from)
    }

    /// All upgrade edges that end at the given kind.
    pub fn upgrades_to<'a>(&'a self, to: &UnitKind) -> impl Iterator<Item = &'a UpgradeEdge> + 'a {
        let to = to.clone();
        self.upgrade_edges.iter().filter(move |e| e.to == to)
    }
}
