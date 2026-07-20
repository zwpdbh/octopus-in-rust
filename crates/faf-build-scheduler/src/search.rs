//! Shared search infrastructure for ECS scheduling algorithms.

use std::collections::HashMap;

use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitEcoStats, UnitKind};
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
use faf_sim_shared::plan::{ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary};
use faf_sim_shared::{BuildTask, EcoSnapshot};
use faf_solver::{plan_completion_with_tasks, PlanResult};

use crate::request::{EcoTarget, SearchOptions};
use crate::resources::{CurrentInventory, EconomyState, SearchGoal, StepLog, TaskLog};
use crate::result::{Action, Schedule, ScheduleError};

/// Resource wrapper so the `BlueprintLibrary` can be inserted into a Bevy
/// `World`.
#[derive(Resource)]
pub(crate) struct BlueprintLibraryRef(pub std::sync::Arc<BlueprintLibrary>);

/// A candidate build/upgrade action considered by a search algorithm.
#[derive(Component)]
pub(crate) struct CandidateAction(pub Action);

/// Score attached to a candidate after evaluation. Lower is better.
#[derive(Component)]
pub(crate) struct CandidateScore(pub f64);

/// The search goal.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchTarget {
    Eco(EcoTarget),
    Unit(UnitKind),
}

impl SearchTarget {
    /// True if the goal has been met given the current economy and inventory.
    pub fn is_reached(
        &self,
        current_eco: &EcoSnapshot,
        inventory: &HashMap<UnitKind, u32>,
    ) -> bool {
        match self {
            SearchTarget::Eco(target) => target.is_reached(current_eco),
            SearchTarget::Unit(kind) => inventory.get(kind).copied().unwrap_or(0) > 0,
        }
    }
}

/// Compute the highest technology tier available from the current inventory.
///
/// The tier is determined by the highest engineer owned. If no engineer is
/// present, the tier defaults to [`TechLevel::T1`].
pub(crate) fn compute_current_tech_level(
    inventory: &HashMap<UnitKind, u32>,
) -> faf_blueprints::TechLevel {
    use faf_blueprints::TechLevel;

    inventory
        .iter()
        .filter(|(_, count)| **count > 0)
        .filter_map(|(kind, _)| match kind {
            UnitKind::Engineer(tech) => Some(*tech),
            _ => None,
        })
        .max()
        .unwrap_or(TechLevel::T1)
}

/// Convert the chosen actions into a final `Schedule`.
pub(crate) fn build_schedule(
    economy_state: &EconomyState,
    inventory: &CurrentInventory,
    task_log: &TaskLog,
    step_log: &StepLog,
    goal: &SearchGoal,
) -> Result<Schedule, ScheduleError> {
    if !goal.0.is_reached(&economy_state.current, &inventory.0) {
        // The apply step already checks this before calling `build_schedule`,
        // but keep the guard for robustness.
        return Err(ScheduleError::GoalUnreachable);
    }

    let plan = build_construction_plan(economy_state, task_log);
    Ok(Schedule {
        plan,
        steps: step_log.0.clone(),
        final_eco: economy_state.current,
        total_time_seconds: step_log
            .0
            .last()
            .map(|s| s.finish_time_seconds)
            .unwrap_or(0.0),
    })
}

fn build_construction_plan(economy_state: &EconomyState, task_log: &TaskLog) -> ConstructionPlan {
    let items = task_log
        .0
        .iter()
        .map(|task| ConstructionItem {
            id: task.id,
            builders: task
                .builders
                .iter()
                .map(UnitSummary::from_builder_stats)
                .collect(),
            targets: task
                .targets
                .iter()
                .map(UnitSummary::from_target_stats)
                .collect(),
            start_after: task.start_after,
        })
        .collect();

    ConstructionPlan {
        eco: snapshot_to_initial_settings(&economy_state.initial),
        items,
    }
}

fn snapshot_to_initial_settings(snapshot: &EcoSnapshot) -> EcoInitialSettings {
    EcoInitialSettings {
        production_per_second_mass: MassRate::from_raw(snapshot.production_per_second_mass),
        production_per_second_energy: EnergyRate::from_raw(snapshot.production_per_second_energy),
        maintenance_consumption_per_second_energy: EnergyRate::from_raw(
            snapshot.maintenance_consumption_per_second_energy,
        ),
        mass_storage: Storage::new(
            Mass::from_raw(snapshot.mass_storage),
            Mass::from_raw(snapshot.mass_storage_cap),
        ),
        energy_storage: Storage::new(
            Energy::from_raw(snapshot.energy_storage),
            Energy::from_raw(snapshot.energy_storage_cap),
        ),
    }
}

/// Build a `BuildTask` representing `action` if it is legal in the current
/// inventory.
pub(crate) fn build_task_for_action(
    action: &Action,
    inventory: &HashMap<UnitKind, u32>,
    library: &BlueprintLibrary,
    id: u32,
) -> Option<BuildTask> {
    match action {
        Action::Build { target, builder } => {
            if !has_builder(inventory, builder) {
                return None;
            }
            if !library.can_build(builder, target) {
                return None;
            }
            Some(BuildTask {
                id,
                start_after: Time::from_raw(0.0),
                builders: vec![to_builder_stats(library, builder)?],
                targets: vec![library.unit_eco_stats(target)?],
            })
        }
        Action::Upgrade { from, to, builder } => {
            if !has_builder(inventory, builder) {
                return None;
            }
            let count = *inventory.get(from)?;
            if count == 0 {
                return None;
            }
            // Find an upgrade path from `from` to `to` using `builder`.
            let path = library
                .upgrade_paths(from)
                .iter()
                .find(|p| p.target == *to && p.builders.contains(builder))?;
            let mut target_stats = library.unit_eco_stats(&path.target)?;
            target_stats.unit_id = Some(format!("upgrade {:?} to {:?}", from, to));
            Some(BuildTask {
                id,
                start_after: Time::from_raw(0.0),
                builders: vec![to_builder_stats(library, builder)?],
                targets: vec![target_stats],
            })
        }
    }
}

fn to_builder_stats(library: &BlueprintLibrary, kind: &UnitKind) -> Option<UnitEcoStats> {
    library.to_unit_eco_stats(kind, true)
}

fn has_builder(inventory: &HashMap<UnitKind, u32>, builder: &UnitKind) -> bool {
    inventory.get(builder).copied().unwrap_or(0) > 0
}

/// Update the inventory as if `action` has been performed.
pub(crate) fn apply_action_to_inventory(action: &Action, inventory: &mut HashMap<UnitKind, u32>) {
    match action {
        Action::Build { target, .. } => {
            *inventory.entry(target.clone()).or_insert(0) += 1;
        }
        Action::Upgrade { from, to, .. } => {
            let from_count = inventory.get_mut(from).expect("upgrade from owned unit");
            *from_count = from_count.saturating_sub(1);
            *inventory.entry(to.clone()).or_insert(0) += 1;
        }
    }
}

/// Simulate `action` as the next step from the current search state and
/// return the per-task result.
pub(crate) fn solve_action(
    current_economy: &EcoSnapshot,
    inventory: &HashMap<UnitKind, u32>,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    library: &BlueprintLibrary,
) -> Option<PlanResult> {
    let task = build_task_for_action(action, inventory, library, next_id)?;
    Some(plan_completion_with_tasks(
        current_economy,
        &[task],
        options.simulation_max_time_seconds,
    ))
}
