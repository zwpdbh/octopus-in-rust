//! The `train` command: train a build-time prediction model.

use faf_build_prediction::model::predictor::EcoPredictorConfig;
use faf_build_prediction::{train_with_ndarray, AdamConfig, TrainingConfig, WeightDecayConfig};

use crate::command_line::TrainArgs;

/// Entry point for the `train` command.
pub fn run(args: TrainArgs) {
    let model_config = EcoPredictorConfig::new()
        .with_hidden_size(args.hidden_size)
        .with_dropout(args.dropout);
    let optimizer_config = AdamConfig::new();
    let optimizer_config = if args.weight_decay > 0.0 {
        optimizer_config.with_weight_decay(Some(WeightDecayConfig::new(args.weight_decay as f32)))
    } else {
        optimizer_config
    };
    let config = TrainingConfig::new(model_config, optimizer_config)
        .with_num_epochs(args.epochs)
        .with_batch_size(args.batch_size)
        .with_learning_rate(args.learning_rate)
        .with_time_weight_power(args.time_weight_power);

    train_with_ndarray(&args.output_dir, &args.dataset, config);
}
