//! Policy-gradient training for the direction-only policy.
//!
//! Uses REINFORCE for the eco head and a supervised rush-readiness head: roll
//! out episodes with the current policy, then update the eco direction head and
//! the rush head from shaped rewards.
//!
//! The training backend is selected by Cargo feature. The library default is
//! `cpu` so tests and library users can run without a GPU. The `faf-sim-cli`
//! binary defaults to `cuda` for training performance.
//!
//! Available backends:
//! - `cpu`: `Autodiff<NdArray>`.
//! - `cuda`: `Autodiff<Cuda>`.
//! - `wgpu`: `Autodiff<Wgpu>`.
//!
//! To switch to a non-default backend, disable the default feature and enable
//! the desired one, for example:
//! `cargo run --no-default-features --features cuda --bin faf-sim -- train ...`
//!
//! When multiple backend features are enabled simultaneously (for example in a
//! workspace build), CPU takes precedence so tests can still run on machines
//! without a GPU when both features are accidentally enabled.

#[cfg(all(feature = "cuda", not(feature = "cpu")))]
use burn::backend::{Autodiff, Cuda};
#[cfg(feature = "cpu")]
use burn::backend::{Autodiff, NdArray};
#[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
use burn::backend::{Autodiff, Wgpu};

pub mod config;
pub mod episode;
pub mod math;
pub mod metric;
pub mod policy_training;
pub mod reward;
pub mod rollout;
pub mod trainer;

#[cfg(test)]
mod tests;

pub use config::{TrainConfig, TrainStats};
pub use metric::{EpisodeSummary, FafSimMetrics, TrainEvent};
pub use policy_training::{load_policy, save_policy, train_policy, train_policy_from};
pub use trainer::Trainer;

// Re-export Burn training/renderer types so the CLI can build a renderer without
// depending directly on the `burn` crate and duplicating backend features.
pub use burn::train::metric::{MetricDefinition, MetricEntry, MetricId, NumericEntry};
pub use burn::train::renderer::{
    EvaluationName, EvaluationProgress, MetricState, MetricsRenderer, MetricsRendererEvaluation,
    MetricsRendererTraining, ProgressType, TrainingProgress,
};
pub use burn::train::Interrupter;
pub use burn::train::LearnerSummary;

/// Autodiff backend used for training.
///
/// When multiple backend features are enabled simultaneously, CPU takes
/// precedence so that tests and workspace builds run without requiring a GPU.
/// Explicit CUDA/WGPU builds (the normal CLI default) still use the GPU backend
/// because only that feature is enabled.
#[cfg(feature = "cpu")]
pub type TrainBackend = Autodiff<NdArray>;
#[cfg(all(feature = "cuda", not(feature = "cpu")))]
pub type TrainBackend = Autodiff<Cuda>;
#[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
pub type TrainBackend = Autodiff<Wgpu>;

/// Device used for training.
pub type TrainDevice = burn::tensor::Device<TrainBackend>;
