//! Policy-gradient training for the hierarchical macro-edge + build-power +
//! engineer-squad policy.
//!
//! Uses REINFORCE: roll out episodes with the current policy, then update all
//! three networks jointly from the completion-time reward.

use burn::backend::{Autodiff, NdArray};

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
pub type TrainBackend = Autodiff<NdArray>;

/// Device used for training.
pub type TrainDevice = burn::tensor::Device<TrainBackend>;
