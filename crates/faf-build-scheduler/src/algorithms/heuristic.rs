//! Algorithm-agnostic heuristics for scoring scheduler actions.
//!
//! These helpers measure the efficiency of a concrete action (for example, mass
//! income gained per mass spent, or the technology tier of the resulting
//! engineer). They do not decide *which* actions to take; that belongs to the
//! scheduling algorithm.
#![allow(unused)]
use faf_blueprints::{TechLevel, UnitKind};

use crate::result::Action;

/// Returns the unit kind that an action produces or upgrades into.
pub(crate) fn resulting_unit(action: &Action) -> UnitKind {
    match action {
        Action::Build { target, .. } => target.clone(),
        Action::Upgrade { to, .. } => to.clone(),
    }
}

/// True if the action is a factory upgrade that unlocks `desired_tech`.
pub(crate) fn is_tech_upgrade_to(action: &Action, desired_tech: TechLevel) -> bool {
    let Action::Upgrade { to, .. } = action else {
        return false;
    };
    matches!(to, UnitKind::Factory(t) if *t == desired_tech)
}

/// If the action results in an engineer, returns its technology tier.
pub(crate) fn engineer_tier(action: &Action) -> Option<TechLevel> {
    match resulting_unit(action) {
        UnitKind::Engineer(tier) => Some(tier),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Mass,
    Energy,
}
