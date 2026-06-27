//! Candidate action pools for MCTS rollouts.
//!
//! A rollout decision is a choice from two pools:
//!
//! 1. **Building pool** — units or upgrades that can be started now.
//! 2. **Assist pool** — idle engineers (grouped by tech tier) that can speed up
//!    an active project.
//!
//! The pools are derived from the static [`PlanGraph`] and the current
//! [`GraphState`]. They change only when ownership or builder availability
//! changes, not on every economy tick.

use std::collections::HashSet;

use crate::planner::plan_graph::{PlanEdgeKind, PlanGraph};
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};
use petgraph::visit::EdgeRef;

/// A single selectable action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Candidate {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade {
        /// Unit kind being upgraded.
        from: UnitKind,
        /// Destination unit kind.
        to: UnitKind,
    },
    /// Assign all idle engineers of the given tier to assist an active project.
    Assist(TechLevel),
}

/// Available idle engineers grouped by tech tier.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssistCounts {
    /// Number of idle T1 engineers available to assist.
    pub t1: u32,
    /// Number of idle T2 engineers available to assist.
    pub t2: u32,
    /// Number of idle T3 engineers available to assist.
    pub t3: u32,
}

impl AssistCounts {
    /// Total idle engineers across all tiers.
    pub fn total(&self) -> u32 {
        self.t1 + self.t2 + self.t3
    }

    /// True if no engineers are available to assist.
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Number of idle engineers for a specific tier.
    pub fn get(&self, tier: TechLevel) -> u32 {
        match tier {
            TechLevel::T1 => self.t1,
            TechLevel::T2 => self.t2,
            TechLevel::T3 => self.t3,
            TechLevel::T4 => 0,
        }
    }
}

/// The two candidate pools used by the MLP-guided rollout.
#[derive(Debug, Clone, Default)]
pub struct SelectionPools {
    /// Units that can be built next.
    pub build: Vec<UnitKind>,
    /// Upgrades that can be started next.
    pub upgrade: Vec<(UnitKind, UnitKind)>,
    /// Idle engineers available to assist, grouped by tier.
    pub assist: AssistCounts,
}

impl SelectionPools {
    /// Derive the current candidate pools from the plan graph and state.
    pub fn derive(plan: &PlanGraph, state: &GraphState, units: &Units) -> Self {
        let mut build = HashSet::new();
        let mut upgrade = HashSet::new();

        let active_targets = state.active_target_unit_ids();

        for edge in plan.graph().edge_references() {
            let source = &plan.graph()[edge.source()];
            let target = &plan.graph()[edge.target()];

            // Source must be owned and active; target must not be owned or
            // already under construction.
            if !state.has_completed_unit(source)
                || state.has_completed_unit(target)
                || active_targets.contains(target)
            {
                continue;
            }

            match edge.weight() {
                PlanEdgeKind::Build => {
                    // Source in a build edge is the builder.
                    if is_idle_builder(state, units, source) {
                        build.insert(target.clone());
                    }
                }
                PlanEdgeKind::Upgrade => {
                    // Source in an upgrade edge is the unit being upgraded.
                    if can_upgrade(state, units, source, target) {
                        upgrade.insert((source.clone(), target.clone()));
                    }
                }
            }
        }

        Self {
            build: build.into_iter().collect(),
            upgrade: upgrade.into_iter().collect(),
            assist: derive_assist_counts(state, units),
        }
    }

    /// Return all candidates as a flat list.
    pub fn candidates(&self) -> Vec<Candidate> {
        let mut candidates: Vec<Candidate> = self
            .build
            .iter()
            .cloned()
            .map(Candidate::Build)
            .collect();

        candidates.extend(
            self.upgrade
                .iter()
                .cloned()
                .map(|(from, to)| Candidate::Upgrade { from, to }),
        );

        for tier in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
            if self.assist.get(tier) > 0 {
                candidates.push(Candidate::Assist(tier));
            }
        }

        candidates
    }

    /// True if there are no candidates at all.
    pub fn is_empty(&self) -> bool {
        self.build.is_empty() && self.upgrade.is_empty() && self.assist.is_empty()
    }
}

/// True if the state has an active, idle builder of the given kind.
fn is_idle_builder(state: &GraphState, units: &Units, kind: &UnitKind) -> bool {
    state
        .idle_builders(units)
        .iter()
        .any(|&id| state.graph[id].unit_id == *kind)
}

/// True if `source` can be upgraded into `target` now.
///
/// The source unit must be active and not already busy, and there must be an
/// idle builder capable of performing the upgrade.
fn can_upgrade(
    state: &GraphState,
    units: &Units,
    source: &UnitKind,
    target: &UnitKind,
) -> bool {
    // Find an active source unit that is not already upgrading or building.
    let source_nodes: Vec<_> = state
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
            .idle_builders(units)
            .iter()
            .any(|&id| state.graph[id].unit_id == *builder_kind)
    })
}

/// Count idle engineers by tech tier.
fn derive_assist_counts(state: &GraphState, units: &Units) -> AssistCounts {
    let mut counts = AssistCounts::default();

    for &id in &state.idle_builders(units) {
        if let UnitKind::Engineer(tier) = state.graph[id].unit_id {
            match tier {
                TechLevel::T1 => counts.t1 += 1,
                TechLevel::T2 => counts.t2 += 1,
                TechLevel::T3 => counts.t3 += 1,
                TechLevel::T4 => {}
            }
        }
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn initial_pools_from_acu() {
        let units = load_units();
        let plan = units.plan_graph(&UnitKind::Pgen(TechLevel::T1)).unwrap();
        let state = GraphState::new(&units, &[UnitKind::Commander]);

        let pools = SelectionPools::derive(&plan, &state, &units);

        // ACU can build T1 factory, mex, and pgen.
        assert!(pools.build.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(pools.build.contains(&UnitKind::Mex(TechLevel::T1)));
        assert!(pools.build.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(pools.upgrade.is_empty());
        assert!(pools.assist.is_empty());
    }

    #[test]
    fn upgrade_pool_appears_when_source_exists() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let plan = units.plan_graph(&goal).unwrap();
        let state = GraphState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Factory(TechLevel::T1),
                UnitKind::Mex(TechLevel::T1),
            ],
        );

        let pools = SelectionPools::derive(&plan, &state, &units);

        // We own Mex_T1 and have an idle engineer, so mex upgrade is a candidate.
        assert!(pools
            .upgrade
            .contains(&(UnitKind::Mex(TechLevel::T1), UnitKind::Mex(TechLevel::T2))));
    }
}
