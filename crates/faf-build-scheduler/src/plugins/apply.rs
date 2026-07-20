//! Apply-best plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::lifecycle::{SchedulerResult, SchedulerSet};
use crate::request::SearchOptions;
use crate::resources::{
    CurrentInventory, CurrentTechLevel, EconomyState, SearchGoal, SearchProgress, StepLog, TaskLog,
};
use crate::result::{ScheduleError, StepResult};
use crate::search::{
    apply_action_to_inventory, build_schedule, build_task_for_action, compute_current_tech_level,
    BlueprintLibraryRef, CandidateAction, CandidateScore,
};

/// Plugin that registers the generic apply step.
///
/// Add this alongside a scheduling-mode plugin (`EcoSchedulingPlugin` or
/// `UnitSchedulingPlugin`). The mode plugin scores candidates in the
/// `EvaluateCandidate` set; this system picks the lowest-scored candidate and
/// commits it in the `Apply` set.
pub struct ApplyPlugin;

impl Plugin for ApplyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_best_system.in_set(SchedulerSet::Apply));
    }
}

/// Apply the lowest-scoring candidate by committing it and updating the search state.
pub(crate) fn apply_best_system(
    mut commands: Commands,
    mut economy: ResMut<EconomyState>,
    mut inventory: ResMut<CurrentInventory>,
    mut tech_level: ResMut<CurrentTechLevel>,
    mut progress: ResMut<SearchProgress>,
    mut task_log: ResMut<TaskLog>,
    mut step_log: ResMut<StepLog>,
    goal: Res<SearchGoal>,
    options: Res<SearchOptions>,
    library: Res<BlueprintLibraryRef>,
    mut result: ResMut<SchedulerResult>,
    candidates: Query<(Entity, &CandidateAction, &CandidateScore)>,
) {
    if progress.done {
        return;
    }

    progress.iteration += 1;

    let library = &*library.0;

    if let Some((best_entity, best_action, _score)) = candidates
        .iter()
        .min_by(|(_, _, a), (_, _, b)| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    {
        let Some(task) =
            build_task_for_action(&best_action.0, &inventory.0, library, progress.next_id)
        else {
            commands.entity(best_entity).despawn();
            if progress.should_terminate(&options) {
                progress.done = true;
                result.result = Some(Err(ScheduleError::GoalUnreachable));
            }
            return;
        };

        apply_action_to_inventory(&best_action.0, &mut inventory.0);
        progress.next_id += 1;

        // Recompute the available tech tier now that the inventory has changed.
        tech_level.0 = compute_current_tech_level(&inventory.0);

        // Run the actual simulator for the chosen action to capture the resulting
        // economy and elapsed time.
        let completion = faf_solver::plan_completion_with_tasks(
            &economy.current,
            &[task.clone()],
            options.simulation_max_time_seconds,
        );
        let final_task = completion.tasks.last().cloned().unwrap_or(completion.total);
        economy.current = final_task.economy.clone();

        let finish_time_seconds = final_task.time_seconds;
        task_log.0.push(task);
        step_log.0.push(StepResult {
            action: best_action.0.clone(),
            finish_time_seconds,
            economy: final_task.economy.clone(),
        });

        commands.entity(best_entity).despawn();

        if goal.0.is_reached(&economy.current, &inventory.0) {
            progress.done = true;
            result.result = Some(build_schedule(
                &economy, &inventory, &task_log, &step_log, &goal,
            ));
            return;
        }

        if progress.should_terminate(&options) {
            progress.done = true;
            result.result = Some(Err(ScheduleError::GoalUnreachable));
        }

        // Despawn remaining candidates so the next iteration starts fresh.
        for (entity, _, _) in candidates.iter() {
            if entity != best_entity {
                commands.entity(entity).despawn();
            }
        }
    } else {
        // No candidates: search is stuck.
        progress.done = true;
        result.result = Some(Err(ScheduleError::GoalUnreachable));
    }
}
