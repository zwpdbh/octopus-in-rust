//! Placeholder scheduling algorithm that produces a trivial but valid schedule.

use std::collections::HashMap;

use faf_sim::quantities::Time;
use faf_sim::runtime::{BuildTask, EcoSnapshot};
use faf_sim::units::{role_of, BlueprintLibrary, UnitKind, UnitRole};
use faf_sim::{plan_completion_with_tasks, CompletionResult};

use crate::algorithms::SchedulingAlgorithm;
use crate::request::{EcoScheduleRequest, EcoTarget, UnitScheduleRequest};
use crate::result::{Action, Schedule, ScheduleError, StepResult};
use crate::util::eco_snapshot_to_runtime_state;

/// Trivial wiring-only scheduler.
#[derive(Debug, Default)]
pub struct Placeholder;

impl Placeholder {
    /// Create a new placeholder scheduler.
    pub fn new() -> Self {
        Self
    }
}

impl SchedulingAlgorithm for Placeholder {
    fn name(&self) -> &'static str {
        "placeholder"
    }

    fn schedule_eco(
        &self,
        library: &BlueprintLibrary,
        request: &EcoScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        let mut inventory = count_inventory(&request.initial_inventory);
        let mut current_eco = request.initial_eco;
        let mut tasks = Vec::new();
        let mut step_results = Vec::new();
        let mut task_id = 1u32;
        let start = std::time::Instant::now();

        while !request.target.is_reached(&current_eco) {
            if start.elapsed().as_secs_f64() >= request.options.max_search_seconds {
                break;
            }

            let best = pick_fastest_eco_action(library, &inventory, &request.target)
                .ok_or(ScheduleError::GoalUnreachable)?;

            let task = build_task(library, &best.target, &best.builder, &inventory, task_id)?;
            let result = simulate_task(
                &current_eco,
                &task,
                request.options.simulation_max_time_seconds,
            )?;

            current_eco = result.economy;
            *inventory.entry(best.target.clone()).or_insert(0) += 1;
            tasks.push(task);
            step_results.push(StepResult {
                action: Action::Build {
                    target: best.target,
                    builder: best.builder,
                },
                finish_time_seconds: result.time_seconds,
                economy: current_eco,
            });
            task_id += 1;
        }

        finalize_schedule(&request.initial_eco, tasks, step_results)
    }

    fn schedule_unit(
        &self,
        library: &BlueprintLibrary,
        request: &UnitScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        let mut inventory = count_inventory(&request.initial_inventory);
        let mut current_eco = request.initial_eco;
        let mut tasks = Vec::new();
        let mut step_results = Vec::new();
        let mut task_id = 1u32;

        if inventory.get(&request.target).copied().unwrap_or(0) > 0 {
            return finalize_schedule(&request.initial_eco, tasks, step_results);
        }

        let builder = library
            .builders_for(&request.target)
            .into_iter()
            .next()
            .ok_or(ScheduleError::NoLegalBuilder {
                target: request.target.clone(),
            })?;

        ensure_builder(
            library,
            &builder,
            &mut inventory,
            &mut current_eco,
            &mut tasks,
            &mut step_results,
            &mut task_id,
            request,
        )?;
        build_one(
            library,
            &request.target,
            &builder,
            &mut inventory,
            &mut current_eco,
            &mut tasks,
            &mut step_results,
            &mut task_id,
            request,
        )?;

        finalize_schedule(&request.initial_eco, tasks, step_results)
    }
}

#[derive(Debug)]
struct Candidate {
    target: UnitKind,
    builder: UnitKind,
    build_time: f64,
}

fn count_inventory(items: &[UnitKind]) -> HashMap<UnitKind, u32> {
    let mut counts = HashMap::new();
    for item in items {
        *counts.entry(item.clone()).or_insert(0) += 1;
    }
    counts
}

fn is_eco_kind(kind: &UnitKind) -> bool {
    matches!(
        role_of(kind),
        UnitRole::MassExtractor
            | UnitRole::PowerGenerator
            | UnitRole::EnergyStorage
            | UnitRole::CappedMassExtractor
    )
}

fn helps_target(stats: &faf_sim::runtime::UnitEcoStats, goal: &EcoTarget) -> bool {
    goal.mass_production
        .is_some_and(|_| stats.production_per_second_mass > 0.0)
        || goal
            .energy_production
            .is_some_and(|_| stats.production_per_second_energy > 0.0)
        || goal
            .mass_storage_cap
            .is_some_and(|_| stats.mass_storage > 0.0)
        || goal
            .energy_storage_cap
            .is_some_and(|_| stats.energy_storage > 0.0)
}

fn pick_fastest_eco_action(
    library: &BlueprintLibrary,
    inventory: &HashMap<UnitKind, u32>,
    goal: &EcoTarget,
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for (builder_kind, count) in inventory {
        if *count == 0 {
            continue;
        }
        for target in library.buildable_by(builder_kind) {
            if !is_eco_kind(&target) {
                continue;
            }
            let stats = library.to_unit_eco_stats(&target, false)?;
            if !helps_target(&stats, goal) {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|b| stats.build_time < b.build_time)
            {
                best = Some(Candidate {
                    target: target.clone(),
                    builder: builder_kind.clone(),
                    build_time: stats.build_time,
                });
            }
        }
    }
    best
}

fn build_task(
    library: &BlueprintLibrary,
    target: &UnitKind,
    builder: &UnitKind,
    inventory: &HashMap<UnitKind, u32>,
    id: u32,
) -> Result<BuildTask, ScheduleError> {
    let builder_count = inventory.get(builder).copied().unwrap_or(0) as usize;
    let builder_stats =
        library
            .to_unit_eco_stats(builder, true)
            .ok_or_else(|| ScheduleError::NoLegalBuilder {
                target: target.clone(),
            })?;
    let target_stats =
        library
            .to_unit_eco_stats(target, false)
            .ok_or_else(|| ScheduleError::NoLegalBuilder {
                target: target.clone(),
            })?;

    Ok(BuildTask {
        id,
        start_after: Time::from_raw(0.0),
        builders: vec![builder_stats; builder_count.max(1)],
        targets: vec![target_stats],
    })
}

fn simulate_task(
    initial_eco: &EcoSnapshot,
    task: &BuildTask,
    max_time_seconds: f64,
) -> Result<CompletionResult, ScheduleError> {
    let result =
        plan_completion_with_tasks(initial_eco, std::slice::from_ref(task), max_time_seconds);
    if result.total.time_seconds >= max_time_seconds - f64::EPSILON {
        return Err(ScheduleError::SimulationStalled);
    }
    Ok(result.total)
}

#[allow(clippy::too_many_arguments)]
fn build_one(
    library: &BlueprintLibrary,
    target: &UnitKind,
    builder: &UnitKind,
    inventory: &mut HashMap<UnitKind, u32>,
    current_eco: &mut EcoSnapshot,
    tasks: &mut Vec<BuildTask>,
    step_results: &mut Vec<StepResult>,
    task_id: &mut u32,
    request: &UnitScheduleRequest,
) -> Result<(), ScheduleError> {
    let task = build_task(library, target, builder, inventory, *task_id)?;
    let result = simulate_task(
        current_eco,
        &task,
        request.options.simulation_max_time_seconds,
    )?;
    *current_eco = result.economy;
    *inventory.entry(target.clone()).or_insert(0) += 1;
    tasks.push(task);
    step_results.push(StepResult {
        action: Action::Build {
            target: target.clone(),
            builder: builder.clone(),
        },
        finish_time_seconds: result.time_seconds,
        economy: *current_eco,
    });
    *task_id += 1;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ensure_builder(
    library: &BlueprintLibrary,
    builder: &UnitKind,
    inventory: &mut HashMap<UnitKind, u32>,
    current_eco: &mut EcoSnapshot,
    tasks: &mut Vec<BuildTask>,
    step_results: &mut Vec<StepResult>,
    task_id: &mut u32,
    request: &UnitScheduleRequest,
) -> Result<(), ScheduleError> {
    if inventory.get(builder).copied().unwrap_or(0) > 0 {
        return Ok(());
    }

    let prereq = library
        .builders_for(builder)
        .into_iter()
        .next()
        .ok_or(ScheduleError::GoalUnreachable)?;

    ensure_builder(
        library,
        &prereq,
        inventory,
        current_eco,
        tasks,
        step_results,
        task_id,
        request,
    )?;
    build_one(
        library,
        builder,
        &prereq,
        inventory,
        current_eco,
        tasks,
        step_results,
        task_id,
        request,
    )?;
    Ok(())
}

fn finalize_schedule(
    initial_eco: &EcoSnapshot,
    tasks: Vec<BuildTask>,
    step_results: Vec<StepResult>,
) -> Result<Schedule, ScheduleError> {
    let queue = faf_sim::runtime::BuildQueue {
        initial_eco: eco_snapshot_to_runtime_state(initial_eco),
        tasks,
    };
    let result = plan_completion_with_tasks(initial_eco, &queue.tasks, 6000.0);
    if result.total.time_seconds >= 6000.0 - f64::EPSILON {
        return Err(ScheduleError::SimulationStalled);
    }
    Ok(Schedule {
        build_queue: queue,
        total_time_seconds: result.total.time_seconds,
        final_eco: result.total.economy,
        steps: step_results,
    })
}
