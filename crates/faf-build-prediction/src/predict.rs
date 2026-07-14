//! Load a trained model and predict the completion time of a build plan.

use std::path::Path;

use anyhow::{Context, Result};
use burn::backend::NdArray;
use burn::prelude::*;
use burn::record::{CompactRecorder, Recorder};
use faf_sim::runtime::{BuildTask, EcoSnapshot};

use crate::data::normalize::NormalizationParams;
use crate::data::sample::{extract_features, FEATURE_DIM};
use crate::train::TrainingConfig;

/// Result of predicting a single plan.
#[derive(Debug, Clone, Copy)]
pub struct Prediction {
    /// Predicted completion time in seconds.
    pub predicted_time_seconds: f64,
    /// True if the predicted time is below the practical threshold.
    pub is_practical: bool,
}

/// Load a trained model from `artifact_dir` and predict how long `plan` will
/// take starting from `initial_eco`.
pub fn predict(
    artifact_dir: &Path,
    initial_eco: &EcoSnapshot,
    plan: &[BuildTask],
    practical_threshold_seconds: f64,
) -> Result<Prediction> {
    let config_path = artifact_dir.join("config.json");
    let model_path = artifact_dir.join("model");
    let norm_path = artifact_dir.join("norm.json");

    let config: TrainingConfig =
        TrainingConfig::load(config_path.to_str().unwrap()).with_context(|| {
            format!(
                "Failed to load training config from {}",
                config_path.display()
            )
        })?;

    let norm = NormalizationParams::load(&norm_path).with_context(|| {
        format!(
            "Failed to load normalization params from {}",
            norm_path.display()
        )
    })?;

    let device = Default::default();
    let record = CompactRecorder::new()
        .load(model_path.clone().into(), &device)
        .with_context(|| format!("Failed to load model from {}", model_path.display()))?;

    let model = config.model.init::<NdArray>(&device).load_record(record);

    let raw_features = extract_features(initial_eco, plan);
    let normalized = norm.normalize(&raw_features);

    let features = Tensor::<NdArray, 2>::from_data(
        TensorData::new(normalized, [1, FEATURE_DIM]).convert::<f32>(),
        &device,
    );

    let output = model.forward(features);
    let log_time: f32 = output.into_scalar();
    let predicted_time = (log_time as f64).exp();

    Ok(Prediction {
        predicted_time_seconds: predicted_time,
        is_practical: predicted_time < practical_threshold_seconds,
    })
}
