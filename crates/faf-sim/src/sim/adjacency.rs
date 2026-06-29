//! Adjacency bonus model for Supreme Commander / FAF economy.
//!
//! In FAF, production buildings receive bonuses when storage buildings are
//! placed next to them. The simulator currently models the two production
//! bonuses that matter most for build-order planning:
//!
//! - **Mass storage → mass extractor**: each adjacent mass storage increases
//!   the extractor's mass production.
//! - **Energy storage → power generator**: each adjacent energy storage
//!   increases the generator's energy production.
//!
//! The exact percentage depends on the footprint sizes of both buildings. For
//! the common case of a size-4 producer (mass extractor or power generator)
//! adjacent to a size-4 storage building, each storage contributes **+12.5%**,
//! capped at **4 adjacent storages** (**+50%** total). This matches the
//! behaviour described on the SupCom Fandom adjacency-bonus page.
//!
//! The module keeps the calculation in one place so that mexes and pgens use
//! the same formula. Higher-level code decides how adjacency counts are set
//! (e.g., energy storages are assigned to the least-capped pgen, and a
//! `CapT2Mex`/`CapT3Mex` upgrade represents a mex already surrounded by four
//! mass storages).

use std::collections::HashMap;

use crate::sim::state::NodeId;
use crate::units::UnitKind;

/// Maximum number of storage buildings that can contribute to one producer.
pub const MAX_ADJACENCY: usize = 4;

/// Production bonus per adjacent storage for same-sized (size-4) buildings.
pub const BONUS_PER_STORAGE: f64 = 0.125;

/// Kind of production adjacency bonus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdjacencyKind {
    /// Mass storage adjacent to a mass extractor.
    Mass,
    /// Energy storage adjacent to a power generator.
    Energy,
}

impl AdjacencyKind {
    /// Returns `true` if `kind` is a producer that benefits from this adjacency.
    pub fn is_producer(self, kind: &UnitKind) -> bool {
        match self {
            AdjacencyKind::Mass => {
                matches!(
                    kind,
                    UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex
                )
            }
            AdjacencyKind::Energy => matches!(kind, UnitKind::Pgen(_)),
        }
    }
}

/// Calculate the production multiplier for a producer with `count` adjacent
/// storage buildings.
///
/// For same-sized buildings this is `1.0 + 0.125 * min(count, 4)`.
pub fn production_multiplier(count: usize) -> f64 {
    1.0 + BONUS_PER_STORAGE * (count.min(MAX_ADJACENCY) as f64)
}

/// Tracks how many storage buildings are adjacent to each producer node.
///
/// Two independent maps are kept because a node can never be both a mex and a
/// pgen, but the same tracker instance covers both resource types.
#[derive(Debug, Default, Clone)]
pub struct AdjacencyTracker {
    mass: HashMap<NodeId, usize>,
    energy: HashMap<NodeId, usize>,
}

impl AdjacencyTracker {
    /// Create an empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current adjacency count for a producer node.
    pub fn count(&self, kind: AdjacencyKind, producer: NodeId) -> usize {
        let map = match kind {
            AdjacencyKind::Mass => &self.mass,
            AdjacencyKind::Energy => &self.energy,
        };
        map.get(&producer).copied().unwrap_or(0)
    }

    /// Set the adjacency count for a producer node, capping at [`MAX_ADJACENCY`].
    pub fn set(&mut self, kind: AdjacencyKind, producer: NodeId, count: usize) {
        let map = match kind {
            AdjacencyKind::Mass => &mut self.mass,
            AdjacencyKind::Energy => &mut self.energy,
        };
        map.insert(producer, count.min(MAX_ADJACENCY));
    }

    /// Increment the adjacency count for a producer node, capping at [`MAX_ADJACENCY`].
    pub fn add(&mut self, kind: AdjacencyKind, producer: NodeId) {
        let count = self.count(kind, producer);
        self.set(kind, producer, count + 1);
    }

    /// Assign one adjacency point from a completed storage building to the
    /// least-capped matching producer among `candidates`.
    ///
    /// `is_producer` is called for each candidate node and should return `true`
    /// when the node is an active producer that can accept this kind of
    /// adjacency.
    ///
    /// Returns the producer node that received the adjacency, if any.
    pub fn assign_to_least_capped(
        &mut self,
        kind: AdjacencyKind,
        candidates: impl Iterator<Item = NodeId>,
        mut is_producer: impl FnMut(NodeId) -> bool,
    ) -> Option<NodeId> {
        let mut best: Option<NodeId> = None;
        let mut best_count = usize::MAX;

        for node_id in candidates {
            if !is_producer(node_id) {
                continue;
            }
            let count = self.count(kind, node_id);
            if count < MAX_ADJACENCY && count < best_count {
                best = Some(node_id);
                best_count = count;
            }
        }

        if let Some(target) = best {
            self.add(kind, target);
            Some(target)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplier_caps_at_four() {
        assert!((production_multiplier(0) - 1.0).abs() < 1e-9);
        assert!((production_multiplier(1) - 1.125).abs() < 1e-9);
        assert!((production_multiplier(4) - 1.5).abs() < 1e-9);
        assert!((production_multiplier(10) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn tracker_caps_additions() {
        let mut tracker = AdjacencyTracker::new();
        let producer = NodeId::new(0);

        for _ in 0..6 {
            tracker.add(AdjacencyKind::Energy, producer);
        }

        assert_eq!(tracker.count(AdjacencyKind::Energy, producer), MAX_ADJACENCY);
    }
}
