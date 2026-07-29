//! The `predict` command: estimate completion time with the analytical solver.

use faf_sim::GameEcoMetrics;
use faf_sim_shared::BuildQueue;
use faf_solver::plan_completion_with_tasks;

use crate::command_line::{PredictMode, PredictShared, PredictSolverArgs};
use crate::util::read_json;

/// Entry point for the `predict` command.
pub fn run(mode: PredictMode) {
    let PredictMode::Solver(args) = mode;
    run_solver(args);
}

fn run_solver(args: PredictSolverArgs) {
    let (eco, queue) = load_plan_and_eco(&args.shared);

    let plan_result = plan_completion_with_tasks(&eco, &queue.tasks, args.max_time_seconds);

    println!("{:?}", plan_result);
}

fn load_plan_and_eco(shared: &PredictShared) -> (GameEcoMetrics, BuildQueue) {
    let queue = read_json::<BuildQueue>(&shared.plan);
    let eco = match &shared.eco {
        Some(path) => read_json::<GameEcoMetrics>(path),
        None => queue.initial_eco,
    };
    (eco, queue)
}
