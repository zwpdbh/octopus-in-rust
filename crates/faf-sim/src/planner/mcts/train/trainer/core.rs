//! Trainer for the hierarchical policy networks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::grad_clipping::GradientClippingConfig;
use burn::optim::{Adam, AdamConfig};
use rand::rngs::ThreadRng;

use super::super::config::TrainConfig;
use super::super::episode::BuildTrajectory;
use super::super::observer::TrainingObserver;
use super::super::{TrainBackend, TrainDevice};
use crate::planner::mcts::macro_net::PolicyBundle;

/// Concrete optimizer type returned by `AdamConfig::init` for a full policy bundle.
pub type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;

pub struct Trainer {
    pub(crate) model: PolicyBundle<TrainBackend>,
    pub(crate) best_model: Option<PolicyBundle<TrainBackend>>,
    pub(crate) best_trajectory: Option<BuildTrajectory>,
    pub(crate) optimizer: AdamOptimizer,
    pub(crate) config: TrainConfig,
    pub(crate) device: TrainDevice,
    pub(crate) rng: ThreadRng,
    /// Optional progress observer. Kept `pub(crate)` so that higher-level entry
    /// points can move it to a fresh `Trainer` during supervised fine-tuning.
    pub(crate) observer: Option<Box<dyn TrainingObserver>>,
    /// Shared flag that can be set from another thread to request a graceful
    /// stop at the next episode boundary.
    pub(crate) stop_requested: Arc<AtomicBool>,
}

impl Trainer {
    /// Create a new trainer with random initialization.
    pub fn new(config: TrainConfig, num_edges: usize) -> Self {
        let device: TrainDevice = Default::default();
        let model = PolicyBundle::new(&device, num_edges);
        Self::from_model(config, model)
    }

    /// Create a trainer that continues from an existing model.
    pub fn from_model(config: TrainConfig, model: PolicyBundle<TrainBackend>) -> Self {
        let device: TrainDevice = Default::default();
        let optimizer = {
            let adam = AdamConfig::new();
            let adam = if let Some(clip) = config.grad_clip {
                adam.with_grad_clipping(Some(GradientClippingConfig::Norm(clip)))
            } else {
                adam
            };
            adam.init()
        };
        Self {
            model,
            best_model: None,
            best_trajectory: None,
            optimizer,
            config,
            device,
            rng: rand::rng(),
            observer: None,
            stop_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Attach a progress observer.
    pub fn with_observer(mut self, observer: impl TrainingObserver + 'static) -> Self {
        self.observer = Some(Box::new(observer));
        self
    }

    /// Request a graceful stop at the next episode boundary.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
    }

    pub(crate) fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed)
            || self.observer.as_ref().map_or(false, |o| o.should_stop())
    }

    /// Consume the trainer and return the trained policy bundle.
    pub fn into_model(self) -> PolicyBundle<TrainBackend> {
        self.model
    }
}
