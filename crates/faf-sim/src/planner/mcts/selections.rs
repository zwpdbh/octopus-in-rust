//! Selectable action options for MCTS rollouts.
//!
//! A rollout decision is a choice from a set of [`SelectionOption`]s. The full
//! space of possible options at any state is huge (every buildable unit, every
//! upgrade pair, every active project), so this module narrows it down to the
//! subset that is both:
//!
//! 1. Reachable on the static [`PlanGraph`] from units the state currently owns.
//! 2. Executable given the current [`GraphState`] (idle builders, active
//!    projects, etc.).
//!
//! The resulting [`SelectionPools`] is therefore a plan-graph-constrained,
//! state-dependent subset of all possible [`SelectionOption`]s. It changes only
//! when ownership or builder availability changes, not on every economy tick.

use std::collections::HashSet;

use petgraph::visit::EdgeRef;

use crate::planner::plan_graph::{PlanEdgeKind, PlanGraph};
use crate::planner::search::SimAction;
use crate::sim::{GraphState, NodeId, UnitNodeState};
use crate::units::{UnitKind, Units};

/// A single selectable action option.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SelectionOption {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade {
        /// Unit kind being upgraded.
        from: UnitKind,
        /// Destination unit kind.
        to: UnitKind,
    },
    /// Assist an active project. The specific builders are resolved when the
    /// option is converted into a concrete simulator command.
    Assist(NodeId),
}

/// A wrapper around the legal [`SelectionOption`]s for the current state.
///
/// This is not the full set of all possible [`SelectionOption`]s. It is the
/// subset reachable on the [`PlanGraph`] from the units currently owned in
/// `state`, combined with assist options for projects that are actively being
/// built or upgraded.
#[derive(Debug, Clone, Default)]
pub struct SelectionPools {
    options: Vec<SelectionOption>,
}

impl SelectionPools {
    /// Derive the current legal selection options from the plan graph and state.
    ///
    /// Walks every edge in `plan` and keeps only those whose source is owned and
    /// active, whose target is not yet owned or under construction, and for
    /// which a capable idle builder exists. Assist options are added for every
    /// active project when at least one idle engineer is available.
    pub fn new(plan: &PlanGraph, state: &GraphState, units: &Units) -> Self {
        let mut options: Vec<SelectionOption> = Vec::new();
        let mut seen: HashSet<SelectionOption> = HashSet::new();

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
                        let opt = SelectionOption::Build(target.clone());
                        if seen.insert(opt.clone()) {
                            options.push(opt);
                        }
                    }
                }
                PlanEdgeKind::Upgrade => {
                    // Source in an upgrade edge is the unit being upgraded.
                    if can_upgrade(state, units, source, target) {
                        let opt = SelectionOption::Upgrade {
                            from: source.clone(),
                            to: target.clone(),
                        };
                        if seen.insert(opt.clone()) {
                            options.push(opt);
                        }
                    }
                }
            }
        }

        if has_idle_engineer(state, units) {
            options.extend(
                state
                    .graph
                    .graph
                    .node_weights()
                    .filter(|n| {
                        matches!(
                            n.state,
                            UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
                        )
                    })
                    .map(|n| SelectionOption::Assist(n.id)),
            );
        }

        Self { options }
    }

    /// All legal selection options.
    pub fn options(&self) -> &[SelectionOption] {
        &self.options
    }

    /// True if there are no options at all.
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
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
fn can_upgrade(state: &GraphState, units: &Units, source: &UnitKind, target: &UnitKind) -> bool {
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

/// True if there is at least one idle engineer in the state.
fn has_idle_engineer(state: &GraphState, units: &Units) -> bool {
    state
        .idle_builders(units)
        .iter()
        .any(|&id| matches!(state.graph[id].unit_id, UnitKind::Engineer(_)))
}

impl SelectionOption {
    /// Convert this option into a concrete simulator command if it is executable.
    pub(crate) fn to_sim_action(&self, state: &GraphState, units: &Units) -> Option<SimAction> {
        match self {
            SelectionOption::Build(target) => {
                let builder = find_idle_builder(state, units, target)?;
                Some(SimAction::Build {
                    unit_id: target.clone(),
                    builder,
                })
            }
            SelectionOption::Upgrade { from, to } => {
                let (old_node, builder) = find_upgrade_parts(state, units, from, to)?;
                Some(SimAction::Upgrade {
                    target_unit_id: to.clone(),
                    old_node,
                    builder,
                })
            }
            SelectionOption::Assist(target) => {
                // Verify the target is still an active project.
                if !matches!(
                    state.graph[*target].state,
                    UnitNodeState::Constructing { .. } | UnitNodeState::Upgrading { .. }
                ) {
                    return None;
                }
                // Assign all idle engineers to the project.
                let builders: Vec<NodeId> = state
                    .idle_builders(units)
                    .into_iter()
                    .filter(|&id| matches!(state.graph[id].unit_id, UnitKind::Engineer(_)))
                    .collect();
                if builders.is_empty() {
                    return None;
                }
                Some(SimAction::Assist {
                    project_node: *target,
                    builders,
                })
            }
        }
    }
}

/// Find an idle builder node capable of building `target`.
fn find_idle_builder(state: &GraphState, units: &Units, target: &UnitKind) -> Option<NodeId> {
    state
        .idle_builders(units)
        .into_iter()
        .find(|&id| units.can_build(&state.graph[id].unit_id, target))
}

/// Find an active source unit and an idle builder for an upgrade.
fn find_upgrade_parts(
    state: &GraphState,
    units: &Units,
    from: &UnitKind,
    to: &UnitKind,
) -> Option<(NodeId, NodeId)> {
    let recipe = units.upgrade_recipes(from).iter().find(|r| r.to == *to)?;

    let old_node = state
        .graph
        .graph
        .node_weights()
        .find(|n| n.is_active() && n.unit_id == *from)
        .map(|n| n.id)?;

    let builder = state
        .idle_builders(units)
        .into_iter()
        .find(|&id| recipe.builder_options.contains(&state.graph[id].unit_id))?;

    Some((old_node, builder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn initial_pools_from_acu() {
        let units = load_units();
        let plan = units.plan_graph(&UnitKind::Pgen(TechLevel::T1)).unwrap();
        let state = GraphState::new(&units, &[UnitKind::Commander]);

        let pools = SelectionPools::new(&plan, &state, &units);

        // ACU can build T1 factory, mex, and pgen.
        assert!(pools
            .options()
            .contains(&SelectionOption::Build(UnitKind::Factory(TechLevel::T1))));
        assert!(pools
            .options()
            .contains(&SelectionOption::Build(UnitKind::Mex(TechLevel::T1))));
        assert!(pools
            .options()
            .contains(&SelectionOption::Build(UnitKind::Pgen(TechLevel::T1))));
        assert!(!pools
            .options()
            .iter()
            .any(|o| matches!(o, SelectionOption::Upgrade { .. })));
        assert!(!pools.is_empty());
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

        let pools = SelectionPools::new(&plan, &state, &units);

        // We own Mex_T1 and have an idle engineer, so mex upgrade is a candidate.
        assert!(pools.options().contains(&SelectionOption::Upgrade {
            from: UnitKind::Mex(TechLevel::T1),
            to: UnitKind::Mex(TechLevel::T2),
        }));
    }
}
