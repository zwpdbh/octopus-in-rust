//! The `predict` command: estimate completion time with a model or the solver.

use faf_build_prediction::{predict as predict_model, Prediction};
use faf_sim::sim::BuildQueue;

use crate::command_line::{PredictMode, PredictNnArgs, PredictShared, PredictSolverArgs};
use crate::util::{eco_snapshot_from_runtime_state, read_json};

/// Entry point for the `predict` command.
pub fn run(mode: PredictMode) {
    match mode {
        PredictMode::Nn(args) => run_nn(args),
        PredictMode::Solver(args) => run_solver(args),
    }
}

fn run_nn(args: PredictNnArgs) {
    let (eco, queue) = load_plan_and_eco(&args.shared);

    match predict_model(args.model_dir.as_ref(), &eco, &queue.tasks) {
        Ok(Prediction {
            predicted_time_seconds,
        }) => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "predicted_time_seconds": predicted_time_seconds.round() as u64,
                }))
                .expect("serialize prediction")
            );
        }
        Err(e) => {
            eprintln!("Prediction failed: {e}");
            std::process::exit(1);
        }
    }
}

fn run_solver(args: PredictSolverArgs) {
    let (eco, queue) = load_plan_and_eco(&args.shared);

    let result = faf_sim::plan_completion_with_tasks(&eco, &queue.tasks, args.max_time_seconds);
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

fn load_plan_and_eco(shared: &PredictShared) -> (faf_sim::EcoSnapshot, BuildQueue) {
    let queue = read_json::<BuildQueue>(&shared.plan);
    let eco = match &shared.eco {
        Some(path) => read_json::<faf_sim::EcoSnapshot>(path),
        None => eco_snapshot_from_runtime_state(&queue.initial_eco),
    };
    (eco, queue)
}
