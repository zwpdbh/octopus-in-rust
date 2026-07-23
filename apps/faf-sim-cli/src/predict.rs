//! The `predict` command: estimate completion time with the analytical solver.

use faf_sim_shared::{BuildQueue, EcoSnapshot};
use faf_solver::plan_completion_with_tasks;

use crate::command_line::{PredictMode, PredictShared, PredictSolverArgs};
use crate::util::{eco_snapshot_from_runtime_state, read_json};

/// Entry point for the `predict` command.
pub fn run(mode: PredictMode) {
    let PredictMode::Solver(args) = mode;
    run_solver(args);
}

fn run_solver(args: PredictSolverArgs) {
    let (eco, queue) = load_plan_and_eco(&args.shared);

    let result = plan_completion_with_tasks(&eco, &queue.tasks, args.max_time_seconds);
    let tasks: Vec<_> = result
        .tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "predicted_time_seconds": t.time_seconds.round() as u64,
                "economy": t.economy,
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "predicted_time_seconds": result.total.time_seconds.round() as u64,
            "economy": result.total.economy,
            "tasks": tasks,
        }))
        .expect("serialize prediction")
    );
}

fn load_plan_and_eco(shared: &PredictShared) -> (EcoSnapshot, BuildQueue) {
    let queue = read_json::<BuildQueue>(&shared.plan);
    let eco = match &shared.eco {
        Some(path) => read_json::<EcoSnapshot>(path),
        None => eco_snapshot_from_runtime_state(&queue.initial_eco),
    };
    (eco, queue)
}
