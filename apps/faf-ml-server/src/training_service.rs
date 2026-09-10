//! Real training service: runs the SSD training loop (`faf-ml-model::train`)
//! on a dedicated thread and publishes events into a server-side **registry**,
//! so a run's status is monitorable at any time (`GET /api/training/status`)
//! and WebSocket viewers can attach/detach freely — a disconnect never
//! interrupts training (real runs take tens of minutes).
//!
//! The dummy curve generator that used to live here is gone; the dummy's
//! protocol shapes (`TrainingServerMessage::Metrics` per batch + epoch-end
//! point with `valid_loss`) are preserved so the web monitor and MCP tools
//! see no difference.

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use faf_ml_core::{
    TrainingCommand, TrainingConfig, TrainingMetricsPoint, TrainingRunResult,
    TrainingServerMessage, TrainingStatus,
};
use faf_ml_model::train::{train, TrainControl, TrainEvent, TrainParams};
use tokio::sync::broadcast;

use crate::state::AppState;

/// Live registry entry for the current/last training run (`result == None`
/// means a run is active — which is also the busy guard).
pub struct RunState {
    pub config: TrainingConfig,
    pub started_at: DateTime<Utc>,
    pub status: TrainingStatus,
    /// Every metrics point so far (replayed to newly attached viewers).
    pub points: Vec<TrainingMetricsPoint>,
    /// Live event feed for WS viewers (created with the run).
    pub events_tx: broadcast::Sender<TrainingServerMessage>,
    /// Command channel into the training thread (cloneable, so any attached
    /// viewer — not just the starter — can pause/resume/stop).
    pub cmd_tx: std::sync::mpsc::Sender<TrainingCommand>,
    pub result: Option<TrainingRunResult>,
}

/// Shared throttle: post-batch sleep duration in batches/sec (≤ 0 = unlimited).
type SpeedLimit = Arc<Mutex<f64>>;

pub struct TrainingService;

impl TrainingService {
    /// Start a real training run. Fails when a run is already active.
    /// Viewers attach through the registry (`AppState::training_run`).
    pub fn run(state: &AppState, config: TrainingConfig, speed: f64) -> anyhow::Result<()> {
        let (events_tx, _) = broadcast::channel(1024);
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TrainingCommand>();
        {
            let mut guard = state
                .training_run
                .lock()
                .expect("training registry poisoned");
            if guard.as_ref().is_some_and(|r| r.result.is_none()) {
                return Err(anyhow!("a training run is already active"));
            }
            *guard = Some(RunState {
                config: config.clone(),
                started_at: Utc::now(),
                status: TrainingStatus::Running,
                points: Vec::new(),
                events_tx: events_tx.clone(),
                cmd_tx,
                result: None,
            });
        }

        let registry = state.training_run.clone();
        let params = TrainParams {
            data: state.data_dir.as_ref().clone(),
            out: state.data_dir.join("runs"),
            epochs: config.epochs,
            batch: config.batch_size,
            lr: config.lr,
            valid_fraction: config.valid_fraction,
            max_batches: config.max_batches,
        };
        let cpu = config.cpu;
        thread::spawn(move || run_training_thread(registry, params, cpu, speed, cmd_rx, events_tx));
        Ok(())
    }
}

/// Emit helper: record into the registry AND broadcast to live viewers.
fn emit(
    registry: &Arc<Mutex<Option<RunState>>>,
    events_tx: &broadcast::Sender<TrainingServerMessage>,
    msg: TrainingServerMessage,
) {
    {
        let mut guard = registry.lock().expect("training registry poisoned");
        if let Some(run) = guard.as_mut() {
            match &msg {
                TrainingServerMessage::Metrics(point) => run.points.push(point.clone()),
                TrainingServerMessage::Status(status) => run.status = status.clone(),
                TrainingServerMessage::Error(_) | TrainingServerMessage::Finished => {}
            }
        }
    }
    let _ = events_tx.send(msg);
}

fn set_result(registry: &Arc<Mutex<Option<RunState>>>, result: TrainingRunResult) {
    let mut guard = registry.lock().expect("training registry poisoned");
    if let Some(run) = guard.as_mut() {
        run.result = Some(result);
    }
}

/// The training thread: translate `TrainEvent`s into protocol messages and
/// the command channel into `TrainControl`.
fn run_training_thread(
    registry: Arc<Mutex<Option<RunState>>>,
    params: TrainParams,
    cpu: bool,
    speed: f64,
    cmd_rx: std::sync::mpsc::Receiver<TrainingCommand>,
    events_tx: broadcast::Sender<TrainingServerMessage>,
) {
    let mut seq = 0u64;
    let speed_limit: SpeedLimit = Arc::new(Mutex::new(speed));
    // (paused, aborted) behind a mutex so `control` can be `Fn`.
    let control_state = Arc::new(Mutex::new((false, false)));

    let emit_status = |status: TrainingStatus| {
        emit(&registry, &events_tx, TrainingServerMessage::Status(status));
    };

    let mut on_event = |event: TrainEvent| {
        seq += 1;
        match event {
            TrainEvent::Batch {
                epoch,
                batch,
                total_batches,
                cls_loss,
                bbox_loss,
                total_loss,
            } => emit(
                &registry,
                &events_tx,
                TrainingServerMessage::Metrics(TrainingMetricsPoint {
                    seq,
                    epoch,
                    batch,
                    total_batches,
                    train_loss: total_loss as f64,
                    cls_loss: cls_loss as f64,
                    bbox_loss: bbox_loss as f64,
                    valid_loss: None,
                    map: None,
                }),
            ),
            TrainEvent::EpochEnd {
                epoch,
                total_epochs: _,
                train_cls,
                train_bbox,
                valid_cls,
                valid_bbox,
            } => emit(
                &registry,
                &events_tx,
                TrainingServerMessage::Metrics(TrainingMetricsPoint {
                    seq,
                    epoch,
                    batch: 0, // epoch-eval point (progress line shows latest batch point anyway)
                    total_batches: 0,
                    train_loss: (train_cls + train_bbox) as f64,
                    cls_loss: train_cls as f64,
                    bbox_loss: train_bbox as f64,
                    valid_loss: Some((valid_cls + valid_bbox) as f64),
                    map: None,
                }),
            ),
            TrainEvent::Done {
                run_dir,
                duration_secs,
            } => {
                set_result(
                    &registry,
                    TrainingRunResult::Done {
                        run_dir: run_dir.display().to_string(),
                        duration_secs,
                    },
                );
                emit_status(TrainingStatus::Done { duration_secs });
            }
        }
        // Post-batch throttle (0 or negative = unlimited).
        let limit = *speed_limit.lock().expect("speed mutex poisoned");
        if limit > 0.0 {
            thread::sleep(Duration::from_secs_f64(1.0 / limit));
        }
    };

    let control = {
        let control_state = control_state.clone();
        let speed_limit = speed_limit.clone();
        move || {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    TrainingCommand::Pause => {
                        control_state.lock().expect("control mutex poisoned").0 = true;
                        emit_status(TrainingStatus::Paused);
                    }
                    TrainingCommand::Resume => {
                        control_state.lock().expect("control mutex poisoned").0 = false;
                        emit_status(TrainingStatus::Running);
                    }
                    TrainingCommand::Stop => {
                        control_state.lock().expect("control mutex poisoned").1 = true;
                    }
                    TrainingCommand::SetSpeed { batches_per_sec } => {
                        *speed_limit.lock().expect("speed mutex poisoned") = batches_per_sec;
                    }
                }
            }
            let (paused, aborted) = *control_state.lock().expect("control mutex poisoned");
            if aborted {
                TrainControl::Abort
            } else if paused {
                TrainControl::Pause
            } else {
                TrainControl::Continue
            }
        }
    };

    let result = if cpu {
        train::<faf_ml_model::CpuAdB>(&params, &mut on_event, &control)
    } else {
        train::<faf_ml_model::AdB>(&params, &mut on_event, &control)
    };
    if let Err(err) = result {
        let error = format!("{err:#}");
        set_result(
            &registry,
            TrainingRunResult::Failed {
                error: error.clone(),
            },
        );
        emit_status(TrainingStatus::Failed { error });
    }
}
