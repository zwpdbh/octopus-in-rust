//! High-level training entry points and model persistence.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use burn::module::Module;
use burn::record::{CompactRecorder, Recorder};

use super::config::{TrainConfig, TrainStats};
use super::metric::metrics::FafSimMetrics;
use super::trainer::Trainer;
use super::{TrainBackend, TrainDevice};
use crate::planner::core::Goal;
use crate::planner::policy::macro_net::PolicyBundle;
use crate::units::Units;
use burn::train::Interrupter;

/// Save a trained policy bundle to disk.
pub fn save_policy(
    model: &PolicyBundle<TrainBackend>,
    path: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create model dir: {e}"))?;
    }
    let recorder = CompactRecorder::new();
    recorder
        .record(model.clone().into_record(), path.to_path_buf())
        .map_err(|e| format!("failed to save model: {e}"))
}

/// Load a trained policy bundle from disk.
pub fn load_policy(path: &std::path::Path) -> Result<PolicyBundle<TrainBackend>, String> {
    let device: TrainDevice = Default::default();
    let recorder = CompactRecorder::new();
    let record = recorder
        .load(path.to_path_buf(), &device)
        .map_err(|e| format!("failed to load model: {e}"))?;
    let model = PolicyBundle::new(&device).load_record(record);
    Ok(model)
}

/// Train a policy for `goal` and return the final model, best-seen model, and
/// training statistics.
///
/// `metrics` is a Burn-style metric bundle that receives per-episode events and
/// renders progress through any `MetricsRenderer`. Pass a no-op renderer if live
/// output is not needed.
///
/// `stop_flag`, when provided, is shared with the trainer so an outside actor
/// can request a graceful stop at the next episode boundary. `interrupter` is
/// the Burn interrupter used by renderers such as the built-in TUI to request a
/// graceful stop.
pub fn train_policy(
    units: &Units,
    goal: &Goal,
    config: TrainConfig,
    metrics: FafSimMetrics,
    stop_flag: Option<Arc<AtomicBool>>,
    interrupter: Interrupter,
) -> (
    PolicyBundle<TrainBackend>,
    Option<PolicyBundle<TrainBackend>>,
    TrainStats,
) {
    let mut trainer = Trainer::new(config)
        .with_metrics(metrics)
        .with_interrupter(interrupter);
    if let Some(flag) = stop_flag {
        trainer.stop_requested = flag;
    }
    let stats = trainer.train(units, goal);

    if let Some(ref mut metrics) = trainer.metrics {
        let _ = metrics.on_end(None);
    }

    let best_model = trainer.best_model.take();
    let model = trainer.into_model();
    (model, best_model, stats)
}

/// Continue training an existing policy for `goal`.
///
/// See [`train_policy`] for `stop_flag` semantics.
pub fn train_policy_from(
    model: PolicyBundle<TrainBackend>,
    units: &Units,
    goal: &Goal,
    config: TrainConfig,
    metrics: FafSimMetrics,
    stop_flag: Option<Arc<AtomicBool>>,
    interrupter: Interrupter,
) -> (
    PolicyBundle<TrainBackend>,
    Option<PolicyBundle<TrainBackend>>,
    TrainStats,
) {
    let mut trainer = Trainer::from_model(config, model)
        .with_metrics(metrics)
        .with_interrupter(interrupter);
    if let Some(flag) = stop_flag {
        trainer.stop_requested = flag;
    }
    trainer.best_model = Some(trainer.model.clone());
    let stats = trainer.train(units, goal);

    if let Some(ref mut metrics) = trainer.metrics {
        let _ = metrics.on_end(None);
    }

    let best_model = trainer.best_model.take();
    let model = trainer.into_model();
    (model, best_model, stats)
}
