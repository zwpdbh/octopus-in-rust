//! Algorithm-agnostic heuristics for scoring scheduler actions.
//!
//! These helpers measure the efficiency of a concrete action (for example, mass
//! income gained per mass spent, or the technology tier of the resulting
//! engineer). They do not decide *which* actions to take; that belongs to the
//! scheduling algorithm.

use faf_blueprints::{BlueprintLibrary, TechLevel, UnitKind};
use faf_sim_shared::EcoSnapshot;
use faf_solver::CompletionResult;

use crate::result::Action;

/// Returns the unit kind that an action produces or upgrades into.
pub(crate) fn resulting_unit(action: &Action) -> UnitKind {
    match action {
        Action::Build { target, .. } => target.clone(),
        Action::Upgrade { to, .. } => to.clone(),
    }
}

/// True if the action is an upgrade whose target is exactly `desired_tech`.
pub(crate) fn is_tech_upgrade_to(action: &Action, desired_tech: TechLevel) -> bool {
    let Action::Upgrade { to, .. } = action else {
        return false;
    };
    faf_blueprints::tech_level_of(to) == Some(desired_tech)
}

/// Mass income efficiency: additional mass produced per mass spent on the
/// action. Returns `None` if the action does not increase mass income or has no
/// mass cost.
pub(crate) fn mass_income_efficiency(
    current: &EcoSnapshot,
    completion: &CompletionResult,
    action: &Action,
    library: &BlueprintLibrary,
) -> Option<f64> {
    resource_efficiency(current, completion, action, library, ResourceKind::Mass)
}

/// Energy income efficiency: additional energy produced per mass spent on the
/// action. Returns `None` if the action does not increase energy income or has
/// no mass cost.
pub(crate) fn energy_income_efficiency(
    current: &EcoSnapshot,
    completion: &CompletionResult,
    action: &Action,
    library: &BlueprintLibrary,
) -> Option<f64> {
    resource_efficiency(current, completion, action, library, ResourceKind::Energy)
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

/// Compute `delta_resource / mass_cost` for a resource.
fn resource_efficiency(
    current: &EcoSnapshot,
    completion: &CompletionResult,
    action: &Action,
    library: &BlueprintLibrary,
    resource: ResourceKind,
) -> Option<f64> {
    let resulting = resulting_unit(action);
    let mass_cost = library
        .build_cost(&resulting)
        .map(|c| c.mass)
        .unwrap_or(0.0)
        .max(0.0);
    if mass_cost <= 0.0 {
        return None;
    }

    let delta = match resource {
        ResourceKind::Mass => {
            completion.economy.production_per_second_mass.value()
                - current.production_per_second_mass.value()
        }
        ResourceKind::Energy => {
            completion.economy.production_per_second_energy.value()
                - current.production_per_second_energy.value()
        }
    };

    if delta <= 0.0 {
        return None;
    }

    Some(delta / mass_cost)
}
