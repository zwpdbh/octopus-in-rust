//! Top-level completion-time computation for single-task and sequential plans.

use faf_blueprints::UnitEcoStats;
use faf_sim_shared::{BuildTask, EcoSnapshot};

use crate::sequential::factor::effective_factor;
use crate::sequential::state::{SolverState, EPS};

/// Result of solving a plan: when it finishes and what the economy looks like at
/// that point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompletionResult {
    pub time_seconds: f64,
    pub economy: EcoSnapshot,
}

/// Result of solving a multi-task plan, including a per-task breakdown.
#[derive(Debug, Clone)]
pub struct PlanResult {
    /// Aggregate result for the whole plan.
    pub total: CompletionResult,
    /// Result after each individual task finishes. A stalled task is recorded
    /// with `time_seconds == max_time_seconds`.
    pub tasks: Vec<CompletionResult>,
}

impl CompletionResult {
    /// Natural completion (or a clean max-time cap).
    fn finished(state: &SolverState, max_time_seconds: f64) -> Self {
        Self {
            time_seconds: state.time.min(max_time_seconds),
            economy: state.to_snapshot(),
        }
    }

    /// The plan cannot finish: zero build power, a permanent stall, or the
    /// solver ran out of time. Report the cap as the completion time.
    fn stalled(state: &SolverState, max_time_seconds: f64) -> Self {
        Self {
            time_seconds: max_time_seconds,
            economy: state.to_snapshot(),
        }
    }
}

/// Compute the completion time and final economy of a single task.
///
/// This is a convenience wrapper around [`plan_completion_result`] for the
/// common case of exactly one task.
pub fn single_task_completion_result(
    initial_eco: &EcoSnapshot,
    task: &BuildTask,
    max_time_seconds: f64,
) -> CompletionResult {
    plan_completion_result(initial_eco, std::slice::from_ref(task), max_time_seconds)
}

/// Compute the completion time of a single task.
///
/// Convenience wrapper that returns only the wall-clock time. Use
/// [`single_task_completion_result`] if you also need the final economy.
pub fn single_task_completion_time(
    initial_eco: &EcoSnapshot,
    task: &BuildTask,
    max_time_seconds: f64,
) -> f64 {
    single_task_completion_result(initial_eco, task, max_time_seconds).time_seconds
}

/// Compute the completion time and final economy of a sequence of tasks.
///
/// Tasks are processed in order. After each task finishes, the economy
/// contributions of every target it built are folded into the running state
/// before the next task's `start_after` delay is applied.
pub fn plan_completion_result(
    initial_eco: &EcoSnapshot,
    tasks: &[BuildTask],
    max_time_seconds: f64,
) -> CompletionResult {
    plan_completion_with_tasks(initial_eco, tasks, max_time_seconds).total
}

/// Compute the completion time of a sequence of tasks.
///
/// Convenience wrapper that returns only the wall-clock time. Use
/// [`plan_completion_with_tasks`] if you also need the per-task breakdown.
pub fn plan_completion_time(
    initial_eco: &EcoSnapshot,
    tasks: &[BuildTask],
    max_time_seconds: f64,
) -> f64 {
    plan_completion_with_tasks(initial_eco, tasks, max_time_seconds)
        .total
        .time_seconds
}

/// Compute the completion time and final economy of a sequence of tasks,
/// returning a per-task breakdown.
pub fn plan_completion_with_tasks(
    initial_eco: &EcoSnapshot,
    tasks: &[BuildTask],
    max_time_seconds: f64,
) -> PlanResult {
    let mut state = SolverState::from_snapshot(initial_eco);
    let mut task_results = Vec::with_capacity(tasks.len());

    for task in tasks {
        if state.time >= max_time_seconds - EPS {
            task_results.push(CompletionResult::finished(&state, max_time_seconds));
            return PlanResult {
                total: CompletionResult::finished(&state, max_time_seconds),
                tasks: task_results,
            };
        }

        // Builders are not active until the task's start_after delay has passed.
        // For the first task this is relative to time 0; for later tasks it is
        // relative to the previous task's finish time.
        let idle_ticks = task.start_after.value().ceil() as usize;
        for _ in 0..idle_ticks {
            if state.time >= max_time_seconds {
                task_results.push(CompletionResult::finished(&state, max_time_seconds));
                return PlanResult {
                    total: CompletionResult::finished(&state, max_time_seconds),
                    tasks: task_results,
                };
            }
            state.idle_tick();
        }

        let power: f64 = task.builders.iter().map(|b| b.build_power).sum();
        if power <= EPS {
            task_results.push(CompletionResult::stalled(&state, max_time_seconds));
            return PlanResult {
                total: CompletionResult::stalled(&state, max_time_seconds),
                tasks: task_results,
            };
        }

        let builder_maintenance: f64 = task
            .builders
            .iter()
            .map(|b| b.maintenance_consumption_per_second_energy)
            .sum();
        state.maintenance += builder_maintenance;

        for target in &task.targets {
            if state.time >= max_time_seconds - EPS {
                task_results.push(CompletionResult::finished(&state, max_time_seconds));
                return PlanResult {
                    total: CompletionResult::finished(&state, max_time_seconds),
                    tasks: task_results,
                };
            }

            if target.build_time <= EPS {
                state.add_target_contributions(target);
                continue;
            }

            let mass_drain = power * target.mass_cost / target.build_time;
            let energy_drain = power * target.energy_cost / target.build_time;

            if !solve_target(
                &mut state,
                target,
                power,
                mass_drain,
                energy_drain,
                max_time_seconds,
            ) {
                task_results.push(CompletionResult::stalled(&state, max_time_seconds));
                return PlanResult {
                    total: CompletionResult::stalled(&state, max_time_seconds),
                    tasks: task_results,
                };
            }

            state.add_target_contributions(target);
        }

        task_results.push(CompletionResult::finished(&state, max_time_seconds));
    }

    PlanResult {
        total: CompletionResult::finished(&state, max_time_seconds),
        tasks: task_results,
    }
}

/// Advance the state until a single target completes.
///
/// Returns `false` if the target cannot finish before `max_time_seconds` or if
/// progress stalls (`effective_factor == 0`).
fn solve_target(
    state: &mut SolverState,
    target: &UnitEcoStats,
    power: f64,
    mass_drain: f64,
    energy_drain: f64,
    max_time_seconds: f64,
) -> bool {
    let mut work = target.build_time;
    let mut f = effective_factor(state, mass_drain, energy_drain);

    // Tick while the target has more work remaining than this tick can finish.
    // The simulator checks completion *before* subtracting progress, so one
    // additional tick is counted once work drops to (or below) the per-tick
    // progress.
    while work > f * power {
        if state.time >= max_time_seconds - EPS {
            return false;
        }
        if f <= EPS {
            return false;
        }

        state.target_tick(mass_drain, energy_drain, f);
        work -= f * power;

        f = effective_factor(state, mass_drain, energy_drain);
    }

    // Final detection tick: apply the economy update for this target but do not
    // subtract any more work.
    if state.time >= max_time_seconds - EPS {
        return false;
    }
    if f <= EPS {
        return false;
    }
    state.target_tick(mass_drain, energy_drain, f);

    true
}

/// Given current eco situation and the target to build
/// What is the maximum build power it could hold.
/// It means during the build progress there should be no energy stall
pub fn solve_approriate_builder_power(
    eco_snapshot: &EcoSnapshot,
    target_mass: f64,
    target_energy: f64,
    target_build_time: f64,
) -> f64 {
    let mut bp = 20.0;
    let net_energy_income = eco_snapshot.production_per_second_energy
        - eco_snapshot.maintenance_consumption_per_second_energy
        - eco_snapshot.energy_drain;

    let net_mass_income = eco_snapshot.production_per_second_mass - eco_snapshot.mass_drain;

    loop {
        let rate = target_build_time / bp;
        let mass_drain = target_mass / rate;
        let energy_drain = target_energy / rate;

        if (mass_drain >= net_mass_income.value()) || (energy_drain >= net_energy_income.value()) {
            break;
        }
        bp += 10.0;
    }
    // println!("appropriate bp is: {}", bp);
    return bp;
}
