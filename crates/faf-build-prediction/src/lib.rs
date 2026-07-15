//! Build-time prediction with supervised learning.
//!
//! This crate provides:
//!
//! - `data`: generation of labeled SQLite datasets using the `faf-sim` simulator,
//!   plus a Burn `Dataset` implementation.
//! - `model`: an LSTM sequence regression model predicting `log(completion_time)`.
//! - `train`: the Burn training loop.

pub mod data;
pub mod model;
pub mod predict;
pub mod train;

pub use burn::optim::decay::WeightDecayConfig;
pub use burn::optim::AdamConfig;
pub use data::generator::{
    generate_dataset, DatasetGenerator, DatasetPipeline, GenerationConfig, NormSaved, PipelineNew,
    PipelineStage, SamplesGenerated, SchemaCreated,
};
pub use data::normalize::{Collecting, NormalizationParams, Ready};
pub use data::sample::{
    build_queue, extract_sequence_features, EcoPlanLabel, EcoPlanSample, Simulated, Unsimulated,
    MAX_SEQ_LEN, TASK_FEATURE_DIM,
};
pub use model::predictor::{EcoPredictor, EcoPredictorConfig};
pub use predict::{
    predict, ConfigLoaded, ModelLoaded, NormLoaded, Prediction, PredictorConfigLoaded,
    PredictorModelLoaded, PredictorNormLoaded,
};
pub use train::{train, train_with_ndarray, TrainingConfig};
