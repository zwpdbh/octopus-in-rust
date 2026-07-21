//! Apply lifecycle step.

use bevy_ecs::prelude::*;

use faf_quantities::Time;

use crate::components::{
    BuildPowerComp, BuilderState, CandidateAssignment, ScheduledTask, UnitKindComp,
};
use crate::plugins::lifecycle::SchedulerResult;
use crate::request::SearchOptions;
use crate::resources::{
    CurrentTechLevel, EconomyState, SchedulerClock, SearchGoal, SearchProgress, StepLog, TaskLog,
};
use crate::result::{Action, ScheduleError, StepResult};
use crate::search::{
    build_schedule, build_task_for_action, compute_current_tech_level, BlueprintLibraryRef,
    CandidateAction, CandidateScore, SearchTarget,
};

/// Apply the best-scored candidate by committing it and updating the search state.
pub(crate) fn apply_best_system(
    mut commands: Commands,
    mut economy: ResMut<EconomyState>,
    mut clock: ResMut<SchedulerClock>,
    mut tech_level: ResMut<CurrentTechLevel>,
    mut progress: ResMut<SearchProgress>,
    mut task_log: ResMut<TaskLog>,
    mut step_log: ResMut<StepLog>,
    goal: Res<SearchGoal>,
    options: Res<SearchOptions>,
    library: Res<BlueprintLibraryRef>,
    mut result: ResMut<SchedulerResult>,
    candidates: Query<(
        Entity,
        &CandidateAction,
        &CandidateAssignment,
        &CandidateScore,
    )>,
    mut units: Query<(
        Entity,
        &mut UnitKindComp,
        &mut BuildPowerComp,
        &mut BuilderState,
    )>,
) {
    if progress.done {
        return;
    }

    progress.iteration += 1;

    let library = &*library.0;

    let best = candidates.iter().max_by(|(_, _, _, a), (_, _, _, b)| {
        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
    });

    let Some((best_entity, best_action, best_assignment, _score)) = best else {
        // Candidate generation may have returned early because the goal was
        // already reached. Confirm before reporting failure.
        let reached = match &goal.0 {
            SearchTarget::Eco(target) => target.is_reached(&economy.current),
            SearchTarget::Unit(kind) => units.iter().any(|(_, k, _, _)| k.0 == *kind),
        };
        progress.done = true;
        result.result = Some(if reached {
            build_schedule(&economy, &task_log, &step_log)
        } else {
            Err(ScheduleError::GoalUnreachable)
        });
        return;
    };

    // Verify all assigned builder entities are still idle and collect their
    // builder-focused stats. Another committed task may have claimed one of them
    // in a previous iteration.
    let mut assigned: Vec<(
        Entity,
        faf_blueprints::UnitKind,
        faf_blueprints::UnitEcoStats,
    )> = Vec::new();
    for (entity, kind, _) in best_assignment.0.iter() {
        let Ok((_, kind_comp, _, state)) = units.get(*entity) else {
            commands.entity(best_entity).despawn();
            return;
        };
        if !matches!(state, BuilderState::Idle) {
            commands.entity(best_entity).despawn();
            return;
        }
        // The kind from the assignment should match the live entity; if it
        // doesn't, something went wrong during an upgrade.
        if kind_comp.0 != *kind {
            commands.entity(best_entity).despawn();
            return;
        }
        let stats = library
            .to_unit_eco_stats(kind, true)
            .expect("owned unit has stats");
        assigned.push((*entity, kind.clone(), stats));
    }

    let Some(task) = build_task_for_action(&best_action.0, &assigned, library, progress.next_id)
    else {
        commands.entity(best_entity).despawn();
        if progress.should_terminate(&options) {
            progress.done = true;
            result.result = Some(Err(ScheduleError::GoalUnreachable));
        }
        return;
    };

    // Simulate the task in isolation to get its finish time and resulting economy.
    let completion = faf_solver::plan_completion_with_tasks(
        &economy.current,
        &[task.clone()],
        options.simulation_max_time_seconds,
    );
    let final_task = completion.tasks.last().cloned().unwrap_or(completion.total);
    let finish_time = Time::from_raw(final_task.time_seconds);

    // Commit the task: mark builders busy and record the scheduled task entity.
    let task_id = progress.next_id;
    for (entity, _, _) in &assigned {
        if let Ok((_, _, _, mut state)) = units.get_mut(*entity) {
            *state = BuilderState::Busy {
                task_id,
                until: finish_time,
            };
        }
    }

    commands.spawn(ScheduledTask {
        id: task_id,
        action: best_action.0.clone(),
        assigned_builders: assigned.iter().map(|(e, _, _)| *e).collect(),
        build_task: task.clone(),
        started_at: clock.now,
        expected_finish: finish_time,
    });

    // Advance the global clock to the task finish and apply the results
    // immediately in this sequential scheduler. In a future parallel version
    // this completion step would be deferred until all concurrent tasks at this
    // time have finished.
    clock.now = finish_time;
    economy.current = final_task.economy.clone();

    // Update the world to reflect the finished task.
    match &best_action.0 {
        Action::Build { target, .. } => {
            let build_power = library.build_power(target);
            commands.spawn((
                UnitKindComp(target.clone()),
                BuildPowerComp(build_power),
                BuilderState::Idle,
            ));
        }
        Action::Upgrade { to, .. } => {
            let source_entity = assigned[0].0;
            if let Ok((_, mut kind, mut build_power, _)) = units.get_mut(source_entity) {
                *kind = UnitKindComp(to.clone());
                *build_power = BuildPowerComp(library.build_power(to));
            }
        }
    }

    // Free the assigned builders now that the task is complete in this
    // sequential model.
    for (entity, _, _) in &assigned {
        if let Ok((_, _, _, mut state)) = units.get_mut(*entity) {
            *state = BuilderState::Idle;
        }
    }

    task_log.0.push(task);
    step_log.0.push(StepResult {
        action: best_action.0.clone(),
        finish_time_seconds: finish_time.value(),
        builder_count: assigned.len(),
        economy: final_task.economy.clone(),
    });

    progress.next_id += 1;
    tech_level.0 = compute_current_tech_level(
        units
            .iter()
            .map(|(_, kind, _, _)| kind.0.clone())
            .collect::<Vec<_>>()
            .into_iter(),
    );

    commands.entity(best_entity).despawn();

    // Check whether this committed action already satisfies the goal. The newly
    // spawned unit entity is not visible in the query until commands are flushed
    // at the end of the update, so for unit goals we compare the action's result
    // directly rather than scanning entities.
    let reached = match &goal.0 {
        SearchTarget::Eco(target) => target.is_reached(&economy.current),
        SearchTarget::Unit(target) => {
            let resulting = match &best_action.0 {
                Action::Build { target, .. } => target.clone(),
                Action::Upgrade { to, .. } => to.clone(),
            };
            resulting == *target
        }
    };
    if reached {
        progress.done = true;
        result.result = Some(build_schedule(&economy, &task_log, &step_log));
        return;
    }

    if progress.should_terminate(&options) {
        progress.done = true;
        result.result = Some(Err(ScheduleError::GoalUnreachable));
    }

    // Despawn remaining candidates so the next iteration starts fresh.
    for (entity, _, _, _) in candidates.iter() {
        if entity != best_entity {
            commands.entity(entity).despawn();
        }
    }
}
