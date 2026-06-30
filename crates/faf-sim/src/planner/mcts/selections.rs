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

use crate::planner::core::PlannerConfig;
use crate::planner::plan_graph::{EdgeCategory, PlanEdgeKind, PlanGraph};

use crate::sim::{GraphState, NodeId, UnitNodeState};
use crate::units::{TechLevel, UnitKind, Units};

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
    pub fn new(
        plan: &PlanGraph,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
    ) -> Self {
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
                    if is_idle_builder(state, units, source)
                        && !would_exceed_storage_cap(target, state, config)
                    {
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

/// Number of tech levels tracked by the engineer-squad network.
pub const ENGINEER_TECH_LEVELS: usize = 3;

/// Stable, ordered list of plan-graph edges used by the macro-edge network.
///
/// Each edge corresponds to one index in the macro network's output. Build
/// edges share a target but may differ by source builder; the network selects
/// a concrete edge, and the source requirement is checked when testing legality.
#[derive(Debug, Clone)]
pub struct PlanEdge {
    /// Source node of the edge (builder for [`PlanEdgeKind::Build`], unit being
    /// upgraded for [`PlanEdgeKind::Upgrade`]).
    pub source: UnitKind,
    /// Target node of the edge.
    pub target: UnitKind,
    /// Edge kind.
    pub kind: PlanEdgeKind,
    /// Strategic focus of this edge.
    category: EdgeCategory,
}

impl PlanEdge {
    /// Strategic focus of this edge.
    pub fn category(&self) -> EdgeCategory {
        self.category
    }
}

/// Indexable edge list derived from a [`PlanGraph`].
#[derive(Debug, Clone)]
pub struct PlanEdgeIndex {
    edges: Vec<PlanEdge>,
}

impl PlanEdgeIndex {
    /// Build a stable edge list from the plan graph.
    pub fn new(plan: &PlanGraph) -> Self {
        let edges: Vec<PlanEdge> = plan
            .graph()
            .edge_references()
            .map(|e| {
                let source = plan.graph()[e.source()].clone();
                let target = plan.graph()[e.target()].clone();
                PlanEdge {
                    source: source.clone(),
                    target: target.clone(),
                    kind: *e.weight(),
                    category: EdgeCategory::categorize(&source, &target),
                }
            })
            .collect();
        Self { edges }
    }

    /// Number of edges (and therefore macro-output dimensions).
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// True if the plan graph contains no edges.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Access the edge at `idx`.
    pub fn get(&self, idx: usize) -> Option<&PlanEdge> {
        self.edges.get(idx)
    }

    /// All edges.
    pub fn edges(&self) -> &[PlanEdge] {
        &self.edges
    }

    /// Return a boolean mask indicating which edges are legal in `state`.
    pub fn legal_mask(
        &self,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
    ) -> Vec<bool> {
        self.edges
            .iter()
            .map(|e| is_edge_legal(e, state, units, config))
            .collect()
    }

    /// Return a mask for all edges that belong to `category`.
    pub fn category_mask(&self, category: EdgeCategory) -> Vec<bool> {
        self.edges
            .iter()
            .map(|e| e.category() == category)
            .collect()
    }

    /// Return a mask for edges that are both legal in `state` and belong to
    /// `category`.
    pub fn legal_mask_for_category(
        &self,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
        category: EdgeCategory,
    ) -> Vec<bool> {
        self.edges
            .iter()
            .map(|e| e.category() == category && is_edge_legal(e, state, units, config))
            .collect()
    }

    /// Return a mask over [`EdgeCategory::ALL`] indicating which categories have
    /// at least one legal edge in `state`.
    pub fn legal_category_mask(
        &self,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
    ) -> Vec<bool> {
        EdgeCategory::ALL
            .iter()
            .map(|c| {
                self.edges
                    .iter()
                    .any(|e| e.category() == *c && is_edge_legal(e, state, units, config))
            })
            .collect()
    }

    /// Convert a legal edge index into a [`SelectionOption`].
    ///
    /// Returns `None` if the edge is illegal or the concrete source/builder is
    /// no longer available.
    pub fn to_selection_option(
        &self,
        idx: usize,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
    ) -> Option<SelectionOption> {
        let edge = self.edges.get(idx)?;
        if !is_edge_legal(edge, state, units, config) {
            return None;
        }
        match edge.kind {
            PlanEdgeKind::Build => Some(SelectionOption::Build(edge.target.clone())),
            PlanEdgeKind::Upgrade => Some(SelectionOption::Upgrade {
                from: edge.source.clone(),
                to: edge.target.clone(),
            }),
        }
    }

    /// Find the index of the first edge that resolves to the given selection
    /// option and is currently legal.
    pub fn find_edge_for_option(
        &self,
        option: &SelectionOption,
        state: &GraphState,
        units: &Units,
        config: &PlannerConfig,
    ) -> Option<usize> {
        self.edges
            .iter()
            .enumerate()
            .find(|(_, e)| {
                let same = match (option, e.kind) {
                    (SelectionOption::Build(t), PlanEdgeKind::Build) => *t == e.target,
                    (SelectionOption::Upgrade { from, to }, PlanEdgeKind::Upgrade) => {
                        *from == e.source && *to == e.target
                    }
                    _ => false,
                };
                same && is_edge_legal(e, state, units, config)
            })
            .map(|(i, _)| i)
    }
}

/// True if `edge` can be executed in `state`.
fn is_edge_legal(
    edge: &PlanEdge,
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
) -> bool {
    let active_targets = state.active_target_unit_ids();

    // Source must be owned and active; target must not already be owned or
    // under construction.
    if !state.has_completed_unit(&edge.source)
        || state.has_completed_unit(&edge.target)
        || active_targets.contains(&edge.target)
    {
        return false;
    }

    match edge.kind {
        PlanEdgeKind::Build => {
            is_idle_builder(state, units, &edge.source)
                && !would_exceed_storage_cap(&edge.target, state, config)
        }
        PlanEdgeKind::Upgrade => can_upgrade(state, units, &edge.source, &edge.target),
    }
}

/// True if building `target` would exceed the configured storage cap.
fn would_exceed_storage_cap(target: &UnitKind, state: &GraphState, config: &PlannerConfig) -> bool {
    match target {
        UnitKind::EnergyStorage => {
            state.count_active_energy_storage() >= config.max_energy_storage_count
        }
        _ => false,
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

/// Count idle engineers per tech level [T1, T2, T3].
pub fn idle_engineer_counts(state: &GraphState, units: &Units) -> [usize; ENGINEER_TECH_LEVELS] {
    let mut counts = [0usize; ENGINEER_TECH_LEVELS];
    for &id in &state.idle_builders(units) {
        if let UnitKind::Engineer(t) = &state.graph[id].unit_id {
            if let Some(i) = tech_index(*t) {
                counts[i] += 1;
            }
        }
    }
    counts
}

/// Return the tech-level bucket used by the squad network.
fn tech_index(t: TechLevel) -> Option<usize> {
    match t {
        TechLevel::T1 => Some(0),
        TechLevel::T2 => Some(1),
        TechLevel::T3 => Some(2),
        _ => None,
    }
}

/// Collect idle engineer nodes grouped by tech level, highest build-rate first.
fn idle_engineers_by_tech(
    state: &GraphState,
    units: &Units,
    predicate: &impl Fn(NodeId) -> bool,
) -> [Vec<NodeId>; ENGINEER_TECH_LEVELS] {
    let mut buckets: [Vec<NodeId>; ENGINEER_TECH_LEVELS] = [Vec::new(), Vec::new(), Vec::new()];

    for &id in &state.idle_builders(units) {
        if let UnitKind::Engineer(t) = &state.graph[id].unit_id {
            if let Some(i) = tech_index(*t) {
                if predicate(id) {
                    buckets[i].push(id);
                }
            }
        }
    }

    for bucket in &mut buckets {
        bucket.sort_by(|&a, &b| {
            let rate_a = units
                .def(&state.graph[a].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            let rate_b = units
                .def(&state.graph[b].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            rate_b.total_cmp(&rate_a)
        });
    }

    buckets
}

/// Select a concrete squad of idle engineers for `edge` matching the desired
/// per-tech counts.
///
/// The returned builders are clamped to the number of idle engineers that are
/// actually available and capable of working on this edge. The caller is
/// responsible for updating the shortfall feedback from `desired - assigned`.
pub fn select_squad_for_edge(
    edge: &PlanEdge,
    desired: [usize; ENGINEER_TECH_LEVELS],
    state: &GraphState,
    units: &Units,
) -> Vec<NodeId> {
    let predicate: Box<dyn Fn(NodeId) -> bool> = match edge.kind {
        PlanEdgeKind::Build => {
            Box::new(|id: NodeId| units.can_build(&state.graph[id].unit_id, &edge.target))
        }
        PlanEdgeKind::Upgrade => {
            let recipe = units
                .upgrade_recipes(&edge.source)
                .iter()
                .find(|r| r.to == edge.target)
                .cloned();
            match recipe {
                Some(recipe) => Box::new(move |id: NodeId| {
                    recipe.builder_options.contains(&state.graph[id].unit_id)
                }),
                None => return Vec::new(),
            }
        }
    };

    let buckets = idle_engineers_by_tech(state, units, &predicate);
    let mut squad = Vec::new();
    for (i, bucket) in buckets.iter().enumerate() {
        let take = desired[i].min(bucket.len());
        squad.extend_from_slice(&bucket[..take]);
    }

    // Fallback: if no engineers are available, assign a capable idle non-engineer
    // builder (e.g., the ACU or a factory) so that early-game edges can execute.
    // The squad network only models [T1, T2, T3] engineers, so this fallback is
    // needed for edges whose source is not an engineer.
    if squad.is_empty() {
        let mut fallback: Vec<NodeId> = state
            .idle_builders(units)
            .iter()
            .copied()
            .filter(|&id| {
                predicate(id) && !matches!(state.graph[id].unit_id, UnitKind::Engineer(_))
            })
            .collect();
        fallback.sort_by(|&a, &b| {
            let rate_a = units
                .def(&state.graph[a].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            let rate_b = units
                .def(&state.graph[b].unit_id)
                .map(|d| d.build_rate)
                .unwrap_or(0.0);
            rate_b.total_cmp(&rate_a)
        });
        if let Some(&id) = fallback.first() {
            squad.push(id);
        }
    }

    squad
}

/// Find an active source node of the given kind for an upgrade edge.
pub(crate) fn find_upgrade_source(state: &GraphState, source_kind: &UnitKind) -> Option<NodeId> {
    state
        .graph
        .graph
        .node_weights()
        .find(|n| n.is_active() && n.unit_id == *source_kind)
        .map(|n| n.id)
}

/// Count assigned engineers per tech level from a list of builder node ids.
pub(crate) fn assigned_squad_counts(state: &GraphState, builders: &[NodeId]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for &id in builders {
        if let UnitKind::Engineer(t) = &state.graph[id].unit_id {
            match *t {
                TechLevel::T1 => counts[0] += 1,
                TechLevel::T2 => counts[1] += 1,
                TechLevel::T3 => counts[2] += 1,
                _ => {}
            }
        }
    }
    counts
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

        let config = PlannerConfig::default();
        let pools = SelectionPools::new(&plan, &state, &units, &config);

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

        let config = PlannerConfig::default();
        let pools = SelectionPools::new(&plan, &state, &units, &config);

        // We own Mex_T1 and have an idle engineer, so mex upgrade is a candidate.
        assert!(pools.options().contains(&SelectionOption::Upgrade {
            from: UnitKind::Mex(TechLevel::T1),
            to: UnitKind::Mex(TechLevel::T2),
        }));
    }
}
