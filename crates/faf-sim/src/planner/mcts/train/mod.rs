//! Policy-gradient training for the hierarchical macro-edge + build-power +
//! engineer-squad policy.
//!
//! Uses REINFORCE: roll out episodes with the current policy, then update all
//! four heads jointly from shaped rewards.
//!
//! The training backend is selected by Cargo feature:
//! - `cpu` (default): `Autodiff<NdArray>`.
//! - `cuda`: `Autodiff<Cuda>`.
//! - `wgpu`: `Autodiff<Wgpu>`.
//!
//! To use a GPU backend, disable the default feature and enable the GPU one:
//! `cargo run --no-default-features --features cuda --bin faf-sim -- train ...`

#[cfg(all(feature = "cuda", not(any(feature = "cpu", feature = "wgpu"))))]
use burn::backend::{Autodiff, Cuda};
#[cfg(all(feature = "cpu", not(any(feature = "cuda", feature = "wgpu"))))]
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
#[cfg(all(feature = "cuda", not(any(feature = "cpu", feature = "wgpu"))))]
pub type TrainBackend = Autodiff<Cuda>;
#[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
pub type TrainBackend = Autodiff<Wgpu>;
#[cfg(all(feature = "cpu", not(any(feature = "cuda", feature = "wgpu"))))]
pub type TrainBackend = Autodiff<NdArray>;

/// Device used for training.
pub type TrainDevice = burn::tensor::Device<TrainBackend>;
