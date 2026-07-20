//! Shared search infrastructure for ECS scheduling algorithms.

use std::collections::HashMap;

use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitEcoStats, UnitKind};
use faf_sim::quantities::{Energy, EnergyRate, Mass, MassRate, Storage};
use faf_sim::runtime::{BuildTask, EcoSnapshot};
use faf_sim::{plan_completion_with_tasks, CompletionResult, PlanResult, Time};
use faf_sim_shared::plan::{ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary};

use crate::request::{EcoTarget, SearchOptions};
use crate::result::{Action, Schedule, ScheduleError, StepResult};

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
pub(crate) enum SearchTarget {
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

/// State of a best-first search through the build space.
#[derive(Resource)]
pub(crate) struct SearchState {
    /// Economy snapshot the search started from.
    pub initial_eco: EcoSnapshot,
    /// Current economy snapshot at the end of the chosen steps.
    pub current_eco: EcoSnapshot,
    /// Units currently owned by the player.
    pub inventory: HashMap<UnitKind, u32>,
    /// Search goal.
    pub target: SearchTarget,
    /// Options controlling the search.
    pub options: SearchOptions,
    /// Tasks committed to by the search so far.
    pub tasks: Vec<BuildTask>,
    /// Human-readable steps produced so far.
    pub steps: Vec<StepResult>,
    /// Next available task id.
    pub next_id: u32,
    /// Number of greedy iterations completed.
    pub iteration: usize,
    /// Whether the search has finished.
    pub done: bool,
}

impl SearchState {
    pub fn new(
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: SearchTarget,
        options: SearchOptions,
    ) -> Self {
        Self {
            initial_eco,
            current_eco: initial_eco,
            inventory,
            target,
            options,
            tasks: Vec::new(),
            steps: Vec::new(),
            next_id: 1,
            iteration: 0,
            done: false,
        }
    }

    /// True if the search should stop due to iteration/step limits.
    pub fn should_terminate(&self) -> bool {
        self.iteration >= self.options.max_iterations || self.steps.len() >= self.options.max_steps
    }
}

/// Convert the chosen actions into a final `Schedule`.
pub(crate) fn build_schedule(state: &SearchState) -> Result<Schedule, ScheduleError> {
    if !state
        .target
        .is_reached(&state.current_eco, &state.inventory)
    {
        return Err(ScheduleError::GoalUnreachable);
    }
    let plan = build_construction_plan(state);
    Ok(Schedule {
        plan,
        steps: state.steps.clone(),
        final_eco: state.current_eco,
        total_time_seconds: state
            .steps
            .last()
            .map(|s| s.finish_time_seconds)
            .unwrap_or(0.0),
    })
}

fn build_construction_plan(state: &SearchState) -> ConstructionPlan {
    let items = state
        .tasks
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
        eco: snapshot_to_initial_settings(&state.initial_eco),
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

/// Score a plan completion. Lower is better.
///
/// If the target is reached, the score is the finish time plus a tiny penalty
/// for resource waste. Candidates that do not reach the target are penalised
/// with the simulation cap so that any reaching candidate is always preferred;
/// among non-reaching candidates the score estimates how long the remaining gap
/// would take to close at the projected income.
pub(crate) fn score_result(
    completion: &CompletionResult,
    target: &EcoTarget,
    max_time_seconds: f64,
) -> f64 {
    if target.is_reached(&completion.economy) {
        let mass_waste = (completion.economy.production_per_second_mass
            - target.mass_production.value())
        .max(0.0);
        return completion.time_seconds + mass_waste * 1e-6;
    }

    let mass_gap =
        (target.mass_production.value() - completion.economy.production_per_second_mass).max(0.0);
    let income = completion.economy.production_per_second_mass.max(1.0);

    max_time_seconds + mass_gap / income
}

/// Simulate `action` as the next step from the current search state and
/// return the per-task result.
pub(crate) fn simulate_with_action(
    state: &SearchState,
    action: &Action,
    library: &BlueprintLibrary,
) -> Option<PlanResult> {
    let task = build_task_for_action(action, &state.inventory, library, state.next_id)?;
    Some(plan_completion_with_tasks(
        &state.current_eco,
        &[task],
        state.options.simulation_max_time_seconds,
    ))
}
