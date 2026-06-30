//! Policy-gradient training for the hierarchical macro-edge + build-power +
//! engineer-squad policy.
//!
//! Uses REINFORCE: roll out episodes with the current policy, then update all
//! four heads jointly from shaped rewards.
//!
//! The training backend is selected by Cargo feature. The default is `cpu`,
//! but any single backend can be selected explicitly:
//! - `cpu` (default): `Autodiff<NdArray>`.
//! - `cuda`: `Autodiff<Cuda>`.
//! - `wgpu`: `Autodiff<Wgpu>`.
//!
//! To switch to a non-default backend, disable the default feature and enable
//! the desired one:
//! `cargo run --no-default-features --features cuda --bin faf-sim -- train ...`
//!
//! When multiple backend features are enabled simultaneously (for example in a
//! workspace build), CPU takes precedence so tests run without requiring a GPU.
//! The `faf-sim-cli` package disables the library default and enables `cuda`,
//! so normal training builds still use the GPU backend.

#[cfg(all(feature = "cuda", not(feature = "cpu")))]
use burn::backend::{Autodiff, Cuda};
#[cfg(feature = "cpu")]
use burn::backend::{Autodiff, NdArray};
#[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
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
