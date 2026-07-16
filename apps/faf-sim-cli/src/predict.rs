//! The `predict` command: estimate completion time with a trained model.

use faf_build_prediction::{predict as predict_model, Prediction};
use faf_sim::sim::BuildQueue;

use crate::command_line::PredictArgs;
use crate::util::{eco_snapshot_from_runtime_state, read_json};

/// Entry point for the `predict` command.
pub fn run(args: PredictArgs) {
    let queue = read_json::<BuildQueue>(&args.plan);
    let eco = match args.eco {
        Some(path) => read_json::<faf_sim::EcoSnapshot>(&path),
        None => eco_snapshot_from_runtime_state(&queue.initial_eco),
    };

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
