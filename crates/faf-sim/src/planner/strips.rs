//! STRIPS-style operators for goal-oriented planning.
//!
//! This module provides a minimal classical-planning vocabulary:
//! facts describe what units exist, and operators describe build/upgrade
//! actions with preconditions, add effects, and delete effects.
//!
//! The operator schemas are generated from the `Units` repository so that
//! the planner can reason about the build graph symbolically before any
//! simulation runs.

use crate::units::{UnitKind, Units};

/// A fact in the planning world state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Fact {
    /// The player owns a completed unit of this kind.
    Have(UnitKind),
}

/// A concrete build or upgrade action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StripsAction {
    /// Build `target` using `builder`.
    Build {
        /// Unit to construct.
        target: UnitKind,
        /// Unit that performs the construction.
        builder: UnitKind,
    },
    /// Upgrade `from` into `to` using `builder`.
    Upgrade {
        /// Source unit (consumed by the upgrade).
        from: UnitKind,
        /// Destination unit.
        to: UnitKind,
        /// Unit that performs the upgrade.
        builder: UnitKind,
    },
}

impl StripsAction {
    /// Human-readable action name.
    pub fn display_name(&self) -> String {
        match self {
            StripsAction::Build { target, builder } => {
                format!("build {:?} using {:?}", target, builder)
            }
            StripsAction::Upgrade { from, to, builder } => {
                format!("upgrade {:?} -> {:?} using {:?}", from, to, builder)
            }
        }
    }
}

/// A STRIPS operator: an action together with its preconditions and effects.
#[derive(Debug, Clone)]
pub struct Operator {
    /// Concrete action this operator performs.
    pub action: StripsAction,
    /// Facts that must hold before the action can be applied.
    pub preconditions: Vec<Fact>,
    /// Facts that become true after the action is applied.
    pub add_list: Vec<Fact>,
    /// Facts that become false after the action is applied.
    pub del_list: Vec<Fact>,
}

impl Operator {
    /// Create a build operator for a target unit using a specific builder.
    fn build_operator(target: UnitKind, builder: UnitKind, prereq: Option<&UnitKind>) -> Self {
        let mut preconditions = Vec::with_capacity(2);
        preconditions.push(Fact::Have(builder.clone()));
        if let Some(p) = prereq {
            let fact = Fact::Have(p.clone());
            if !preconditions.contains(&fact) {
                preconditions.push(fact);
            }
        }
        Self {
            action: StripsAction::Build {
                target: target.clone(),
                builder,
            },
            add_list: vec![Fact::Have(target)],
            del_list: Vec::new(),
            preconditions,
        }
    }

    /// Create an upgrade operator from one unit to another using a builder.
    fn upgrade_operator(from: UnitKind, to: UnitKind, builder: UnitKind) -> Self {
        let mut preconditions = vec![Fact::Have(from.clone()), Fact::Have(builder.clone())];
        preconditions.dedup();
        Self {
            action: StripsAction::Upgrade {
                from,
                to: to.clone(),
                builder,
            },
            add_list: vec![Fact::Have(to)],
            del_list: Vec::new(),
            preconditions,
        }
    }
}

/// Generate STRIPS operators from a unit repository.
pub fn build_operators(units: &Units) -> Vec<Operator> {
    let mut ops = Vec::new();

    for (target, recipe) in units.all_build_recipes() {
        for builder in &recipe.builder_options {
            ops.push(Operator::build_operator(
                target.clone(),
                builder.clone(),
                recipe.prereq.as_ref(),
            ));
        }
    }

    for (from, recipes) in units.all_upgrade_recipes() {
        for recipe in recipes {
            for builder in &recipe.builder_options {
                ops.push(Operator::upgrade_operator(
                    from.clone(),
                    recipe.to.clone(),
                    builder.clone(),
                ));
            }
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn operators_include_build_factory_t1() {
        let units = load_units();
        let ops = build_operators(&units);

        let build_t1_factory = ops.iter().find(|op| {
            matches!(
                &op.action,
                StripsAction::Build {
                    target: UnitKind::Factory(TechLevel::T1),
                    builder: UnitKind::Commander,
                }
            )
        });
        assert!(build_t1_factory.is_some());
    }

    #[test]
    fn build_operators_include_prerequisite() {
        let units = load_units();
        let ops = build_operators(&units);

        let build_t3_factory = ops.iter().find(|op| {
            matches!(
                &op.action,
                StripsAction::Build {
                    target: UnitKind::Factory(TechLevel::T3),
                    ..
                }
            )
        });
        let op = build_t3_factory.expect("there should be a way to build T3 factory");
        assert!(op
            .preconditions
            .contains(&Fact::Have(UnitKind::Factory(TechLevel::T2))));
    }

    #[test]
    fn upgrade_operators_exist() {
        let units = load_units();
        let ops = build_operators(&units);

        let upgrade_factory = ops.iter().any(|op| {
            matches!(
                &op.action,
                StripsAction::Upgrade {
                    from: UnitKind::Factory(TechLevel::T1),
                    to: UnitKind::Factory(TechLevel::T2),
                    ..
                }
            )
        });
        assert!(
            upgrade_factory,
            "T1 -> T2 factory upgrade should be an operator"
        );
    }
}
