//! Trainer for the direction-only policy network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::grad_clipping::GradientClippingConfig;
use burn::optim::{Adam, AdamConfig};
use rand::rngs::ThreadRng;

use super::super::config::TrainConfig;
use super::super::episode::BuildTrajectory;
use super::super::metric::metrics::FafSimMetrics;
use super::super::{TrainBackend, TrainDevice};
use crate::planner::mcts::macro_net::PolicyBundle;
use burn::train::Interrupter;

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
    /// Optional Burn-style metric bundle. Kept `pub(crate)` so that higher-level
    /// entry points can move it to a fresh `Trainer` during supervised fine-tuning.
    pub(crate) metrics: Option<FafSimMetrics>,
    /// Shared flag that can be set from another thread to request a graceful
    /// stop at the next episode boundary.
    pub(crate) stop_requested: Arc<AtomicBool>,
    /// Burn interrupter; the built-in TUI renderer sets this when the user
    /// requests a stop.
    pub(crate) interrupter: Interrupter,
}

impl Trainer {
    /// Create a new trainer with random initialization.
    pub fn new(config: TrainConfig) -> Self {
        let device: TrainDevice = Default::default();
        let model = PolicyBundle::new(&device);
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
            metrics: None,
            stop_requested: Arc::new(AtomicBool::new(false)),
            interrupter: Interrupter::new(),
        }
    }

    /// Attach a Burn-style metric bundle.
    pub fn with_metrics(mut self, metrics: FafSimMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Attach a Burn interrupter so the built-in TUI renderer can request a stop.
    pub fn with_interrupter(mut self, interrupter: Interrupter) -> Self {
        self.interrupter = interrupter;
        self
    }

    /// Request a graceful stop at the next episode boundary.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
    }

    pub(crate) fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed) || self.interrupter.should_stop()
    }

    /// Consume the trainer and return the trained policy bundle.
    pub fn into_model(self) -> PolicyBundle<TrainBackend> {
        self.model
    }
}
