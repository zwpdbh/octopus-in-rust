//! Abstract direction-only policy-network interface used by the planner.
//!
//! This module hides the concrete Burn backend and network architecture from
//! [`Planner`][crate::planner::Planner]. Callers choose a [`ValueNetKind`] and
//! the factory turns it into a [`Box<dyn ValueNet>`]; the planner only sees the
//! single inference operation it needs: direction logits.

use crate::planner::core::{PlannerError, ValueNetKind};
use crate::planner::policy::macro_net::HierarchicalPolicyNet;
use crate::planner::policy::train::{TrainBackend, TrainDevice};

/// Trait-object safe interface to the learned direction policy.
///
/// All methods take plain `Vec<f32>` feature vectors and return host-side
/// results, so callers do not need to know which Burn backend or architecture
/// is underneath.
pub trait ValueNet: std::fmt::Debug + Send + Sync {
    /// Evaluate the eco head on a single feature vector.
    fn evaluate_direction(&self, features: Vec<f32>) -> Vec<f32>;

    /// Evaluate the rush head on a single feature vector.
    fn evaluate_rush(&self, features: Vec<f32>) -> f32;

    /// Clone through the trait object.
    fn clone_box(&self) -> Box<dyn ValueNet>;
}

/// Factory that creates [`ValueNet`] instances without exposing concrete types.
pub trait ValueNetFactory: std::fmt::Debug + Send + Sync {
    /// Create a fresh value net.
    fn create(&self) -> Result<Box<dyn ValueNet>, PlannerError>;
}

/// Concrete direction-only policy net backed by [`HierarchicalPolicyNet`].
#[derive(Debug, Clone)]
pub struct MlpValueNet {
    net: HierarchicalPolicyNet<TrainBackend>,
    device: TrainDevice,
}

impl MlpValueNet {
    /// Create a randomly-initialized direction-only policy net.
    pub fn new() -> Self {
        let device: TrainDevice = Default::default();
        let net = HierarchicalPolicyNet::new(&device);
        Self { net, device }
    }

    /// Wrap an existing hierarchical policy network.
    pub fn from_net(net: HierarchicalPolicyNet<TrainBackend>) -> Self {
        let device: TrainDevice = Default::default();
        Self { net, device }
    }

    /// Unwrap the underlying network for training or persistence.
    pub fn into_net(self) -> HierarchicalPolicyNet<TrainBackend> {
        self.net
    }
}

impl Default for MlpValueNet {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueNet for MlpValueNet {
    fn evaluate_direction(&self, features: Vec<f32>) -> Vec<f32> {
        self.net.evaluate_direction(features, &self.device)
    }

    fn evaluate_rush(&self, features: Vec<f32>) -> f32 {
        self.net.evaluate(features, &self.device).1
    }

    fn clone_box(&self) -> Box<dyn ValueNet> {
        Box::new(self.clone())
    }
}

impl ValueNetFactory for ValueNetKind {
    fn create(&self) -> Result<Box<dyn ValueNet>, PlannerError> {
        match self {
            ValueNetKind::Mlp => Ok(Box::new(MlpValueNet::new())),
            ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
                "GNN value net is not yet implemented".to_string(),
            )),
        }
    }
}
