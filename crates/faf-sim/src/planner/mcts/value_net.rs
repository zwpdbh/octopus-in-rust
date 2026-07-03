//! Abstract value-network interface used by the planner.
//!
//! This module hides the concrete Burn backend and network architecture from
//! [`Planner`][crate::planner::Planner]. Callers choose a [`ValueNetKind`] and
//! the factory turns it into a [`Box<dyn ValueNet>`]; the planner only sees the
//! inference operations it actually needs.

use crate::planner::core::{PlannerError, ValueNetKind};
use crate::planner::mcts::macro_net::HierarchicalPolicyNet;
use crate::planner::mcts::train::{TrainBackend, TrainDevice};
use crate::planner::plan_graph::EdgeCategory;

/// Trait-object safe interface to a learned value/policy network.
///
/// All methods take plain `Vec<f32>` feature vectors and return host-side
/// results, so callers do not need to know which Burn backend or architecture
/// is underneath.
pub trait ValueNet: std::fmt::Debug + Send + Sync {
    /// Number of plan-graph edges this network can score.
    fn num_edges(&self) -> usize;

    /// Evaluate the factory-upgrade head on a single feature vector.
    fn evaluate_upgrade(&self, features: Vec<f32>) -> Vec<f32>;

    /// Evaluate the strategic-direction head on a single feature vector.
    fn evaluate_direction(&self, features: Vec<f32>) -> Vec<f32>;

    /// Evaluate the action head for a chosen direction.
    fn evaluate_action(&self, features: Vec<f32>, category: EdgeCategory) -> Vec<f32>;

    /// Evaluate the build-power head for a chosen edge.
    fn evaluate_power(&self, features: Vec<f32>, edge_idx: usize, num_edges: usize) -> f32;

    /// Evaluate the engineer-squad head for a target build power.
    fn evaluate_squad(&self, features: Vec<f32>, power: f32) -> Vec<f32>;

    /// Clone through the trait object.
    fn clone_box(&self) -> Box<dyn ValueNet>;
}

/// Factory that creates [`ValueNet`] instances without exposing concrete types.
pub trait ValueNetFactory: std::fmt::Debug + Send + Sync {
    /// Create a fresh value net for a plan graph with `num_edges` edges.
    fn create(&self, num_edges: usize) -> Result<Box<dyn ValueNet>, PlannerError>;
}

/// Concrete MLP value net backed by [`HierarchicalPolicyNet`].
#[derive(Debug, Clone)]
pub struct MlpValueNet {
    net: HierarchicalPolicyNet<TrainBackend>,
    device: TrainDevice,
}

impl MlpValueNet {
    /// Create a randomly-initialized MLP value net for `num_edges` edges.
    pub fn new(num_edges: usize) -> Self {
        let device: TrainDevice = Default::default();
        let net = HierarchicalPolicyNet::new(&device, num_edges);
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

impl ValueNet for MlpValueNet {
    fn num_edges(&self) -> usize {
        self.net.num_edges()
    }

    fn evaluate_upgrade(&self, features: Vec<f32>) -> Vec<f32> {
        self.net.evaluate_upgrade(features, &self.device)
    }

    fn evaluate_direction(&self, features: Vec<f32>) -> Vec<f32> {
        self.net.evaluate_direction(features, &self.device)
    }

    fn evaluate_action(&self, features: Vec<f32>, category: EdgeCategory) -> Vec<f32> {
        self.net.evaluate_action(features, category, &self.device)
    }

    fn evaluate_power(&self, features: Vec<f32>, edge_idx: usize, num_edges: usize) -> f32 {
        self.net
            .evaluate_power(features, edge_idx, num_edges, &self.device)
    }

    fn evaluate_squad(&self, features: Vec<f32>, power: f32) -> Vec<f32> {
        self.net.evaluate_squad(features, power, &self.device)
    }

    fn clone_box(&self) -> Box<dyn ValueNet> {
        Box::new(self.clone())
    }
}

impl ValueNetFactory for ValueNetKind {
    fn create(&self, num_edges: usize) -> Result<Box<dyn ValueNet>, PlannerError> {
        match self {
            ValueNetKind::Mlp => Ok(Box::new(MlpValueNet::new(num_edges))),
            ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
                "GNN value net is not yet implemented".to_string(),
            )),
        }
    }
}
