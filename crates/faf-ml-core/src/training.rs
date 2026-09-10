//! Training-monitor wire types: the `/ws/training` protocol shared by
//! `faf-ml-server` and `faf-ml-web` (JSON text frames, externally-tagged
//! enums — same style as `faf-sim-protocol`).

use serde::{Deserialize, Serialize};

/// Training-run parameters (mirrors the `faf-ml-train train` CLI args).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingConfig {
    /// Number of epochs to run.
    #[serde(default = "default_epochs")]
    pub epochs: usize,
    /// Batch size (hard-capped at 4 by the GPU buffer limit for the real
    /// detector; the dummy service accepts anything).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Learning rate.
    #[serde(default = "default_lr")]
    pub lr: f64,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            epochs: default_epochs(),
            batch_size: default_batch_size(),
            lr: default_lr(),
        }
    }
}

fn default_epochs() -> usize {
    50
}
fn default_batch_size() -> usize {
    4
}
fn default_lr() -> f64 {
    1e-3
}

/// One chart point, emitted per training batch (epoch-eval fields set only
/// on the epoch-end point).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingMetricsPoint {
    /// Monotonic sequence number (x axis + dedup).
    pub seq: u64,
    pub epoch: usize,
    /// Batch within the epoch (1-based).
    pub batch: usize,
    /// Total batches of the whole run (for the progress line).
    pub total_batches: usize,
    pub train_loss: f64,
    pub cls_loss: f64,
    pub bbox_loss: f64,
    /// Validation loss (epoch-end points only).
    #[serde(default)]
    pub valid_loss: Option<f64>,
    /// Detection mAP (epoch-end points only; dummy until real eval exists).
    #[serde(default)]
    pub map: Option<f64>,
}

/// Browser → server messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrainingClientMessage {
    /// Start a training run (must be the first message on the socket).
    Start {
        config: TrainingConfig,
        /// Dummy tick rate in batches per wall-clock second.
        speed: f64,
    },
    /// Runtime command for a running job.
    Command(TrainingCommand),
}

/// Runtime commands for a training thread.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TrainingCommand {
    Pause,
    Resume,
    Stop,
    SetSpeed { batches_per_sec: f64 },
}

/// Lifecycle of one training run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TrainingStatus {
    Running,
    Paused,
    Done { duration_secs: u64 },
    Failed { error: String },
}

/// Server → browser messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrainingServerMessage {
    Metrics(TrainingMetricsPoint),
    Status(TrainingStatus),
    Error(String),
    /// Training thread exited cleanly (after `Status::Done`); the server
    /// closes the socket right after.
    Finished,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_fill_missing_fields() {
        let config: TrainingConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, TrainingConfig::default());
        assert_eq!(config.batch_size, 4);
    }

    #[test]
    fn protocol_round_trips() {
        let msg = TrainingServerMessage::Metrics(TrainingMetricsPoint {
            seq: 7,
            epoch: 1,
            batch: 3,
            total_batches: 100,
            train_loss: 0.5,
            cls_loss: 0.3,
            bbox_loss: 0.2,
            valid_loss: None,
            map: Some(0.42),
        });
        let raw = serde_json::to_string(&msg).unwrap();
        let back: TrainingServerMessage = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, msg);

        let cmd = TrainingClientMessage::Command(TrainingCommand::SetSpeed {
            batches_per_sec: 5.0,
        });
        let raw = serde_json::to_string(&cmd).unwrap();
        assert_eq!(
            raw,
            r#"{"type":"command","command":"set_speed","batches_per_sec":5.0}"#
        );
        let back: TrainingClientMessage = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, cmd);
    }
}
