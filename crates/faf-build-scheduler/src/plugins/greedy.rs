//! Greedy algorithm plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::scheduler::{SchedulerResult, SchedulerSet};
use crate::result::{ScheduleError, StepResult};
use crate::search::{
    apply_action_to_inventory, build_schedule, build_task_for_action, BlueprintLibraryRef,
    CandidateAction, CandidateScore, SearchState,
};

/// Plugin form of the greedy algorithm. Add this alongside [`SchedulerPlugin`]
/// and a scheduling-mode plugin such as [`EcoSchedulingPlugin`].
pub struct GreedyPlugin;

impl Plugin for GreedyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, select_best_system.in_set(SchedulerSet::Select));
    }
}

/// Select the lowest-scoring candidate, commit it, and update the search state.
pub(crate) fn select_best_system(
    mut commands: Commands,
    mut state: ResMut<SearchState>,
    library: Res<BlueprintLibraryRef>,
    mut result: ResMut<SchedulerResult>,
    candidates: Query<(Entity, &CandidateAction, &CandidateScore)>,
) {
    if state.done {
        return;
    }

    state.iteration += 1;

    let library = &*library.0;

    if let Some((best_entity, best_action, _score)) = candidates
        .iter()
        .min_by(|(_, _, a), (_, _, b)| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    {
        let Some(task) =
            build_task_for_action(&best_action.0, &state.inventory, library, state.next_id)
        else {
            commands.entity(best_entity).despawn();
            if state.should_terminate() {
                state.done = true;
                result.result = Some(Err(ScheduleError::GoalUnreachable));
            }
            return;
        };

        apply_action_to_inventory(&best_action.0, &mut state.inventory);
        state.next_id += 1;

        // Run the actual simulator for the chosen action to capture the resulting
        // economy and elapsed time.
        let completion = faf_sim::plan_completion_with_tasks(
            &state.current_eco,
            &[task.clone()],
            state.options.simulation_max_time_seconds,
        );
        let final_task = completion.tasks.last().cloned().unwrap_or(completion.total);
        state.current_eco = final_task.economy.clone();

        let finish_time_seconds = final_task.time_seconds;
        state.tasks.push(task);
        state.steps.push(StepResult {
            action: best_action.0.clone(),
            finish_time_seconds,
            economy: final_task.economy.clone(),
        });

        commands.entity(best_entity).despawn();

        if state
            .target
            .is_reached(&state.current_eco, &state.inventory)
        {
            state.done = true;
            result.result = Some(build_schedule(&state));
            return;
        }

        if state.should_terminate() {
            state.done = true;
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
        state.done = true;
        result.result = Some(Err(ScheduleError::GoalUnreachable));
    }
}
