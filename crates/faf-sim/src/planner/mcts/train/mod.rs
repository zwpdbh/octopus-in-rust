//! Policy-gradient training for the hierarchical macro-edge + build-power +
//! engineer-squad policy.
//!
//! Uses REINFORCE: roll out episodes with the current policy, then update all
//! four heads jointly from shaped rewards.
//!
//! The training backend is selected by Cargo feature. The default is `cuda`,
//! but any single backend can be selected explicitly:
//! - `cuda` (default): `Autodiff<Cuda>`.
//! - `cpu`: `Autodiff<NdArray>`.
//! - `wgpu`: `Autodiff<Wgpu>`.
//!
//! To switch to a non-default backend, disable the default feature and enable
//! the desired one:
//! `cargo run --no-default-features --features cpu --bin faf-sim -- train ...`

#[cfg(feature = "cuda")]
use burn::backend::{Autodiff, Cuda};
#[cfg(all(feature = "cpu", not(feature = "cuda")))]
use burn::backend::{Autodiff, NdArray};
#[cfg(all(feature = "wgpu", not(any(feature = "cuda", feature = "cpu"))))]
use burn::backend::{Autodiff, Wgpu};

pub mod config;
pub mod episode;
pub mod math;
pub mod policy;
pub mod reward;
pub mod trainer;

#[cfg(test)]
mod tests;

pub use config::{TrainConfig, TrainStats};
pub use policy::{load_policy, save_policy, train_policy, train_policy_from};
pub use trainer::Trainer;

/// Autodiff backend used for training.
///
/// When multiple backend features are enabled simultaneously, CUDA takes
/// precedence, then WGPU, then CPU. This keeps the build robust against
/// Cargo feature unification.
#[cfg(feature = "cuda")]
pub type TrainBackend = Autodiff<Cuda>;
#[cfg(all(feature = "wgpu", not(feature = "cuda")))]
pub type TrainBackend = Autodiff<Wgpu>;
#[cfg(all(feature = "cpu", not(any(feature = "cuda", feature = "wgpu"))))]
pub type TrainBackend = Autodiff<NdArray>;

/// Device used for training.
pub type TrainDevice = burn::tensor::Device<TrainBackend>;
