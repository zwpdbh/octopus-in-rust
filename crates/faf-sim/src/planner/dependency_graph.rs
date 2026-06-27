//! Goal-oriented dependency graph.
//!
//! Builds a tree rooted at a target unit by backward chaining through STRIPS
//! operators. Each node records the action chosen to satisfy its goal and the
//! subgoals (preconditions) of that action.
//!
//! The tree is finite and acyclic because the planner stops when a goal is
//! already satisfied by the initial state (the ACU) and because the recursion
//! aborts if a goal reappears on the current goal stack.

use std::collections::HashSet;

use crate::planner::strips::{Fact, Operator, StripsAction};
use crate::units::UnitKind;

/// A node in the dependency tree.
#[derive(Debug, Clone)]
pub struct DependencyNode {
    /// Goal fact that this node satisfies.
    pub goal: Fact,
    /// Action chosen to satisfy the goal, if any.
    ///
    /// `None` means the goal was already true in the initial state.
    pub chosen_action: Option<StripsAction>,
    /// Subgoals required by `chosen_action`, in the same order as the
    /// operator's preconditions.
    pub subgoals: Vec<DependencyNode>,
}

impl DependencyNode {
    /// Create a leaf node for a goal that is already satisfied.
    fn leaf(goal: Fact) -> Self {
        Self {
            goal,
            chosen_action: None,
            subgoals: Vec::new(),
        }
    }

    /// Create an internal node with a chosen action and its subgoals.
    fn with_action(goal: Fact, action: StripsAction, subgoals: Vec<DependencyNode>) -> Self {
        Self {
            goal,
            chosen_action: Some(action),
            subgoals,
        }
    }
}

/// A dependency tree rooted at a target unit.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    /// Root node: the target unit's `Have` fact.
    pub root: DependencyNode,
}

impl DependencyGraph {
    /// Collect every unit kind that appears in the dependency tree.
    pub fn all_unit_kinds(&self) -> Vec<UnitKind> {
        let mut kinds = Vec::new();
        let mut seen = HashSet::new();
        self.root.collect_unit_kinds(&mut kinds, &mut seen);
        kinds
    }
}

impl DependencyNode {
    fn collect_unit_kinds(&self, out: &mut Vec<UnitKind>, seen: &mut HashSet<UnitKind>) {
        let Fact::Have(kind) = &self.goal;
        if seen.insert(kind.clone()) {
            out.push(kind.clone());
        }
        for sub in &self.subgoals {
            sub.collect_unit_kinds(out, seen);
        }
    }
}

/// Errors that can occur while building a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyGraphError {
    /// No operator can produce the requested goal.
    NoOperatorForGoal(Fact),
    /// The goal stack detected a cycle.
    CycleDetected(Fact),
}

impl std::fmt::Display for DependencyGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyGraphError::NoOperatorForGoal(fact) => {
                write!(f, "no operator produces {:?}", fact)
            }
            DependencyGraphError::CycleDetected(fact) => {
                write!(f, "dependency cycle detected at {:?}", fact)
            }
        }
    }
}

impl std::error::Error for DependencyGraphError {}

/// Build a dependency tree for `goal` starting from `initial_units`.
///
/// The function returns the first complete tree it finds. If no operator can
/// produce a required subgoal, it returns an error.
pub fn build_dependency_graph(
    goal: UnitKind,
    initial_units: &[UnitKind],
    operators: &[Operator],
) -> Result<DependencyGraph, DependencyGraphError> {
    let initial_facts: HashSet<Fact> = initial_units
        .iter()
        .map(|u| Fact::Have(u.clone()))
        .collect();
    let goal_fact = Fact::Have(goal);
    let root = build_node(&goal_fact, operators, &initial_facts, &mut Vec::new())?;
    Ok(DependencyGraph { root })
}

fn build_node(
    goal: &Fact,
    operators: &[Operator],
    initial_facts: &HashSet<Fact>,
    goal_stack: &mut Vec<Fact>,
) -> Result<DependencyNode, DependencyGraphError> {
    if initial_facts.contains(goal) {
        return Ok(DependencyNode::leaf(goal.clone()));
    }

    if goal_stack.contains(goal) {
        return Err(DependencyGraphError::CycleDetected(goal.clone()));
    }

    goal_stack.push(goal.clone());

    // Find operators whose add-list contains this goal.
    let appropriate: Vec<&Operator> = operators
        .iter()
        .filter(|op| op.add_list.contains(goal))
        .collect();

    if appropriate.is_empty() {
        goal_stack.pop();
        return Err(DependencyGraphError::NoOperatorForGoal(goal.clone()));
    }

    // Try each appropriate operator and return the first fully satisfiable tree.
    for op in appropriate {
        match build_subgoals(&op.preconditions, operators, initial_facts, goal_stack) {
            Ok(subgoals) => {
                goal_stack.pop();
                return Ok(DependencyNode::with_action(
                    goal.clone(),
                    op.action.clone(),
                    subgoals,
                ));
            }
            Err(_) => continue,
        }
    }

    goal_stack.pop();
    Err(DependencyGraphError::NoOperatorForGoal(goal.clone()))
}

fn build_subgoals(
    preconditions: &[Fact],
    operators: &[Operator],
    initial_facts: &HashSet<Fact>,
    goal_stack: &mut Vec<Fact>,
) -> Result<Vec<DependencyNode>, DependencyGraphError> {
    let mut subgoals = Vec::with_capacity(preconditions.len());
    for pre in preconditions {
        let node = build_node(pre, operators, initial_facts, goal_stack)?;
        subgoals.push(node);
    }
    Ok(subgoals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::strips::build_operators;
    use crate::units::{TechLevel, UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn pgen_t1_requires_only_commander() {
        let units = load_units();
        let ops = build_operators(&units);
        let graph =
            build_dependency_graph(UnitKind::Pgen(TechLevel::T1), &[UnitKind::Commander], &ops)
                .expect("pgen t1 should be reachable");

        assert!(matches!(
            graph.root.goal,
            Fact::Have(UnitKind::Pgen(TechLevel::T1))
        ));
        assert!(graph
            .root
            .subgoals
            .iter()
            .any(|n| { matches!(n.goal, Fact::Have(UnitKind::Commander)) }));
    }

    #[test]
    fn factory_t2_requires_factory_t1() {
        let units = load_units();
        let ops = build_operators(&units);
        let graph = build_dependency_graph(
            UnitKind::Factory(TechLevel::T2),
            &[UnitKind::Commander],
            &ops,
        )
        .expect("factory t2 should be reachable");

        // The root action should require Factory(T1).
        let has_t1_prereq = graph
            .root
            .subgoals
            .iter()
            .any(|n| matches!(n.goal, Fact::Have(UnitKind::Factory(TechLevel::T1))));
        assert!(has_t1_prereq, "T2 factory should depend on T1 factory");
    }

    #[test]
    fn monkeylord_reaches_t3_factory() {
        let units = load_units();
        let ops = build_operators(&units);
        let goal = UnitKind::Unique(UnitId("URL0402".to_string()));
        let graph = build_dependency_graph(goal.clone(), &[UnitKind::Commander], &ops)
            .expect("monkeylord should be reachable");

        assert_eq!(graph.root.goal, Fact::Have(goal));

        // Walk the tree and ensure a T3 factory appears somewhere.
        fn contains_t3_factory(node: &DependencyNode) -> bool {
            if matches!(node.goal, Fact::Have(UnitKind::Factory(TechLevel::T3))) {
                return true;
            }
            node.subgoals.iter().any(contains_t3_factory)
        }
        assert!(contains_t3_factory(&graph.root));
    }
}
