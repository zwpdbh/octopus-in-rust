//! Load a trained model and predict the completion time of a build plan.

use std::path::Path;

use anyhow::{Context, Result};
use burn::backend::ndarray::NdArrayDevice;
use burn::backend::NdArray;
use burn::prelude::*;
use burn::record::{CompactRecorder, Recorder};
use faf_sim::runtime::{BuildTask, EcoSnapshot};

use crate::data::normalize::{NormalizationParams, Ready};
use crate::data::sample::{extract_sequence_features, TASK_FEATURE_DIM};
use crate::model::predictor::EcoPredictor;
use crate::train::TrainingConfig;

/// Result of predicting a single plan.
#[derive(Debug, Clone, Copy)]
pub struct Prediction {
    /// Predicted completion time in seconds.
    pub predicted_time_seconds: f64,
}

/// Stage marker: training config has been loaded but normalization params have
/// not yet been loaded.
pub struct ConfigLoaded;

/// Stage marker: normalization params have been loaded but the model has not
/// yet been loaded.
pub struct NormLoaded;

/// Stage marker: the model has been loaded and predictions can be made.
pub struct ModelLoaded;

/// Type-state predictor pipeline.
///
/// The legal order is enforced by the compiler:
///
/// ```rust,ignore
/// PredictorConfigLoaded::load_config(&config_path)?
///     .load_norm(&norm_path)?
///     .load_model(&model_path)?
///     .predict(&initial_eco, &plan, threshold);
/// ```
pub struct PredictorConfigLoaded {
    config: TrainingConfig,
    device: NdArrayDevice,
}

/// Predictor after normalization params have been loaded.
pub struct PredictorNormLoaded {
    config: TrainingConfig,
    norm: NormalizationParams<Ready>,
    device: NdArrayDevice,
}

/// Predictor after the model has been loaded and is ready for inference.
pub struct PredictorModelLoaded {
    norm: NormalizationParams<Ready>,
    model: EcoPredictor<NdArray>,
    device: NdArrayDevice,
}

impl PredictorConfigLoaded {
    /// Load the training config from a JSON file.
    pub fn load_config(path: &Path) -> Result<Self> {
        let config: TrainingConfig = TrainingConfig::load(path.to_str().unwrap())
            .with_context(|| format!("Failed to load training config from {}", path.display()))?;
        let device = Default::default();
        Ok(Self { config, device })
    }

    /// Load normalization params and advance to the next stage.
    pub fn load_norm(self, path: &Path) -> Result<PredictorNormLoaded> {
        let norm = NormalizationParams::load(path).with_context(|| {
            format!(
                "Failed to load normalization params from {}",
                path.display()
            )
        })?;
        Ok(PredictorNormLoaded {
            config: self.config,
            norm,
            device: self.device,
        })
    }
}

impl PredictorNormLoaded {
    /// Load the trained model and advance to the ready-for-inference stage.
    pub fn load_model(self, path: &Path) -> Result<PredictorModelLoaded> {
        let record = CompactRecorder::new()
            .load(path.into(), &self.device)
            .with_context(|| format!("Failed to load model from {}", path.display()))?;

        let model = self
            .config
            .model
            .init::<NdArray>(&self.device)
            .load_record(record);

        Ok(PredictorModelLoaded {
            norm: self.norm,
            model,
            device: self.device,
        })
    }
}

impl PredictorModelLoaded {
    /// Run inference on a build plan.
    ///
    /// This single-task predictor only uses the first task in `plan`.
    pub fn predict(&self, initial_eco: &EcoSnapshot, plan: &[BuildTask]) -> Prediction {
        let raw_sequence = extract_sequence_features(initial_eco, plan);
        let raw_features = raw_sequence
            .first()
            .expect("predict called with an empty plan");
        let normalized = self.norm.normalize(raw_features);

        let features = Tensor::<NdArray, 2>::from_data(
            TensorData::new(normalized, [1, TASK_FEATURE_DIM]).convert::<f32>(),
            &self.device,
        );

        let output = self.model.forward(features);
        let log_time: f32 = output.into_scalar();
        let predicted_time = (log_time as f64).exp();

        Prediction {
            predicted_time_seconds: predicted_time,
        }
    }
}

/// Load a trained model from `artifact_dir` and predict how long `plan` will
/// take starting from `initial_eco`.
///
/// This is a convenience wrapper around the [`PredictorConfigLoaded`] pipeline.
pub fn predict(
    artifact_dir: &Path,
    initial_eco: &EcoSnapshot,
    plan: &[BuildTask],
) -> Result<Prediction> {
    let prediction = PredictorConfigLoaded::load_config(&artifact_dir.join("config.json"))?
        .load_norm(&artifact_dir.join("norm.json"))?
        .load_model(&artifact_dir.join("model"))?
        .predict(initial_eco, plan);
    Ok(prediction)
}
