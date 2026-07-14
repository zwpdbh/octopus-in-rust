//! Build-time prediction with supervised learning.
//!
//! This crate provides:
//!
//! - `data`: generation of labeled SQLite datasets using the `faf-sim` simulator,
//!   plus a Burn `Dataset` implementation.
//! - `model`: a small MLP regression model predicting `log(completion_time)`.
//! - `train`: the Burn training loop.

pub mod data;
pub mod model;
pub mod predict;
pub mod train;

pub use burn::optim::AdamConfig;
pub use data::generator::{generate_dataset, GenerationConfig};
pub use data::normalize::NormalizationParams;
pub use data::sample::{extract_features, EcoPlanLabel, EcoPlanSample, PlanStats, FEATURE_DIM};
pub use model::predictor::{EcoPredictor, EcoPredictorConfig};
pub use predict::{predict, Prediction};
pub use train::{train, train_with_ndarray, TrainingConfig};
