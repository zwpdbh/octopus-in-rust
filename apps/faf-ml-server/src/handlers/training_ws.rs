//! Training routes: `GET /ws/training` (start/attach + live stream) and
//! `GET /api/training/status` (the run registry as JSON).
//!
//! This handler is a pure protocol adapter: the training manager actor
//! (`faf-ml-model::manager`, held in `AppState::training`) owns the run; the
//! handler maps wire types ↔ manager types, replays buffered metrics,
//! forwards live broadcast events, and relays commands. A client disconnect
//! ends only the forwarding — training always continues server-side.

use std::path::Path;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    Json,
};
use faf_ml_core::{
    TrainingClientMessage, TrainingCommand, TrainingConfig, TrainingMetricsPoint,
    TrainingRunResult, TrainingRunStatus, TrainingServerMessage, TrainingStatus,
};
use faf_ml_model::{
    manager::{ManagerCommand, ManagerEvent, Outcome, Phase, RunStatus},
    train::{TrainEvent, TrainParams},
};
use tokio::sync::broadcast;

use crate::{
    error::{Error, Result},
    state::AppState,
};

/// `TrainingConfig` (wire) → `TrainParams` (model); the server injects the
/// store paths.
fn train_params(config: &TrainingConfig, data_dir: &Path) -> TrainParams {
    TrainParams {
        data: data_dir.to_path_buf(),
        dataset: config.dataset.clone(),
        out: data_dir.join("runs"),
        epochs: config.epochs,
        batch: config.batch_size,
        lr: config.lr,
        valid_fraction: config.valid_fraction,
        max_batches: config.max_batches,
        cpu: config.cpu,
    }
}

/// `TrainParams` (model) → `TrainingConfig` (wire), for status responses.
fn training_config(params: &TrainParams) -> TrainingConfig {
    TrainingConfig {
        dataset: params.dataset.clone(),
        epochs: params.epochs,
        batch_size: params.batch,
        lr: params.lr,
        valid_fraction: params.valid_fraction,
        max_batches: params.max_batches,
        cpu: params.cpu,
    }
}

fn wire_command(cmd: TrainingCommand) -> ManagerCommand {
    match cmd {
        TrainingCommand::Pause => ManagerCommand::Pause,
        TrainingCommand::Resume => ManagerCommand::Resume,
        TrainingCommand::Stop => ManagerCommand::Stop,
        TrainingCommand::Reset => ManagerCommand::Reset,
        TrainingCommand::SetSpeed { batches_per_sec } => {
            ManagerCommand::SetSpeed { batches_per_sec }
        }
    }
}

/// Manager run status → wire status (`None` when idle — callers 404/error).
fn wire_status(status: &RunStatus) -> Option<TrainingStatus> {
    match status {
        RunStatus::Idle => None,
        RunStatus::Active { phase, .. } => Some(match phase {
            Phase::Running => TrainingStatus::Running,
            Phase::Pausing => TrainingStatus::Pausing,
            Phase::Paused => TrainingStatus::Paused,
            Phase::Stopping => TrainingStatus::Stopping,
        }),
        RunStatus::Ended { outcome, .. } => Some(match outcome {
            Outcome::Completed { duration_secs, .. } => TrainingStatus::Done {
                duration_secs: *duration_secs,
            },
            Outcome::Stopped { duration_secs, .. } => TrainingStatus::Stopped {
                duration_secs: *duration_secs,
            },
            Outcome::Failed { error } => TrainingStatus::Failed {
                error: error.clone(),
            },
        }),
    }
}

fn wire_result(status: &RunStatus) -> Option<TrainingRunResult> {
    match status {
        RunStatus::Ended { outcome, .. } => Some(match outcome {
            Outcome::Completed {
                run_dir,
                duration_secs,
            } => TrainingRunResult::Done {
                run_dir: run_dir.display().to_string(),
                duration_secs: *duration_secs,
            },
            Outcome::Stopped {
                run_dir,
                duration_secs,
            } => TrainingRunResult::Stopped {
                run_dir: run_dir.display().to_string(),
                duration_secs: *duration_secs,
            },
            Outcome::Failed { error } => TrainingRunResult::Failed {
                error: error.clone(),
            },
        }),
        _ => None,
    }
}

/// One `TrainEvent` → one chart point (`seq` is assigned here, per attached
/// viewer, continuing after the replay). `Paused` carries no metrics.
fn metrics_point(seq: u64, event: &TrainEvent) -> Option<TrainingMetricsPoint> {
    match event {
        TrainEvent::Batch {
            epoch,
            batch,
            total_batches,
            cls_loss,
            bbox_loss,
            total_loss,
        } => Some(TrainingMetricsPoint {
            seq,
            epoch: *epoch,
            batch: *batch,
            total_batches: *total_batches,
            train_loss: *total_loss as f64,
            cls_loss: *cls_loss as f64,
            bbox_loss: *bbox_loss as f64,
            valid_loss: None,
            map: None,
        }),
        TrainEvent::EpochEnd {
            epoch,
            train_cls,
            train_bbox,
            valid_cls,
            valid_bbox,
            ..
        } => Some(TrainingMetricsPoint {
            seq,
            epoch: *epoch,
            batch: 0, // epoch-eval point (progress line shows latest batch point anyway)
            total_batches: 0,
            train_loss: (train_cls + train_bbox) as f64,
            cls_loss: *train_cls as f64,
            bbox_loss: *train_bbox as f64,
            valid_loss: Some((valid_cls + valid_bbox) as f64),
            map: None,
        }),
        TrainEvent::Paused => None,
    }
}

/// `GET /api/training/status` — the current/last run (404 when idle).
pub async fn get_training_status(State(state): State<AppState>) -> Result<Json<TrainingRunStatus>> {
    let snapshot = state.training.snapshot().await.map_err(Error::Internal)?;
    let (config, started_at) = match &snapshot.status {
        RunStatus::Idle => return Err(Error::NotFound),
        RunStatus::Active {
            config, started_at, ..
        }
        | RunStatus::Ended {
            config, started_at, ..
        } => (config.clone(), *started_at),
    };
    let points = snapshot.replay.len();
    let latest = snapshot
        .replay
        .last()
        .and_then(|event| metrics_point(points as u64, event));
    Ok(Json(TrainingRunStatus {
        config: training_config(&config),
        started_at,
        status: wire_status(&snapshot.status).expect("idle handled above"),
        points,
        latest,
        result: wire_result(&snapshot.status),
    }))
}

/// Upgrade an HTTP connection to a WebSocket and view/start a training run.
pub async fn training_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket, state: AppState) {
    use axum::extract::ws::Message;

    // First frame decides: start a new run or attach to the current one.
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<TrainingClientMessage>(&text) {
                    Ok(TrainingClientMessage::Start { config, speed }) => {
                        let params = train_params(&config, &state.data_dir);
                        match state.training.start(params, speed).await {
                            Ok(()) => break,
                            Err(e) => {
                                let _ =
                                    send_json(&mut socket, &TrainingServerMessage::Error(e)).await;
                                return;
                            }
                        }
                    }
                    Ok(TrainingClientMessage::Attach) => break,
                    Ok(TrainingClientMessage::Command(_)) => {
                        let _ = send_json(
                            &mut socket,
                            &TrainingServerMessage::Error(
                                "expected Start or Attach before commands".to_string(),
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &TrainingServerMessage::Error(format!("invalid message: {e}")),
                        )
                        .await;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue,
        }
    }

    // Replay + live subscription, taken atomically by the manager.
    let dump = match state.training.attach().await {
        Ok(dump) => dump,
        Err(e) => {
            let _ = send_json(&mut socket, &TrainingServerMessage::Error(e)).await;
            return;
        }
    };
    let Some(status) = wire_status(&dump.status) else {
        let _ = send_json(
            &mut socket,
            &TrainingServerMessage::Error("no training run".to_string()),
        )
        .await;
        return;
    };
    let mut seq = 0u64;
    for event in &dump.replay {
        seq += 1;
        if let Some(point) = metrics_point(seq, event) {
            if send_json(&mut socket, &TrainingServerMessage::Metrics(point))
                .await
                .is_err()
            {
                return;
            }
        }
    }
    let terminal = matches!(
        &status,
        TrainingStatus::Done { .. }
            | TrainingStatus::Stopped { .. }
            | TrainingStatus::Failed { .. }
    );
    if send_json(&mut socket, &TrainingServerMessage::Status(status))
        .await
        .is_err()
    {
        return;
    }
    if terminal {
        // Attaching to an ended run replays it, then closes like a live end.
        let _ = send_json(&mut socket, &TrainingServerMessage::Finished).await;
        return;
    }

    // Forward live events + relay commands until the client leaves or the
    // run reaches a terminal status.
    let mut events = dump.events;
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        let msg = match event {
                            ManagerEvent::Train(event) => {
                                seq += 1;
                                metrics_point(seq, &event).map(TrainingServerMessage::Metrics)
                            }
                            ManagerEvent::PhaseChanged(phase) => {
                                Some(TrainingServerMessage::Status(match phase {
                                    Phase::Running => TrainingStatus::Running,
                                    Phase::Pausing => TrainingStatus::Pausing,
                                    Phase::Paused => TrainingStatus::Paused,
                                    Phase::Stopping => TrainingStatus::Stopping,
                                }))
                            }
                            ManagerEvent::Ended(outcome) => {
                                Some(TrainingServerMessage::Status(match outcome {
                                    Outcome::Completed { duration_secs, .. } => {
                                        TrainingStatus::Done { duration_secs }
                                    }
                                    Outcome::Stopped { duration_secs, .. } => {
                                        TrainingStatus::Stopped { duration_secs }
                                    }
                                    Outcome::Failed { error } => TrainingStatus::Failed { error },
                                }))
                            }
                            ManagerEvent::Reset => Some(TrainingServerMessage::Reset),
                        };
                        let Some(msg) = msg else { continue };
                        let terminal = matches!(
                            &msg,
                            TrainingServerMessage::Status(
                                TrainingStatus::Done { .. }
                                    | TrainingStatus::Stopped { .. }
                                    | TrainingStatus::Failed { .. }
                            )
                        );
                        if send_json(&mut socket, &msg).await.is_err() {
                            return;
                        }
                        if terminal {
                            let _ = send_json(&mut socket, &TrainingServerMessage::Finished).await;
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<TrainingClientMessage>(&text) {
                            Ok(TrainingClientMessage::Command(cmd)) => {
                                if let Err(e) =
                                    state.training.command(wire_command(cmd)).await
                                {
                                    let _ = send_json(
                                        &mut socket,
                                        &TrainingServerMessage::Error(e),
                                    )
                                    .await;
                                }
                            }
                            Ok(TrainingClientMessage::Start { .. }) => {
                                let _ = send_json(
                                    &mut socket,
                                    &TrainingServerMessage::Error("already started".to_string()),
                                )
                                .await;
                            }
                            Ok(TrainingClientMessage::Attach) => {
                                let _ = send_json(
                                    &mut socket,
                                    &TrainingServerMessage::Error("already attached".to_string()),
                                )
                                .await;
                            }
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &TrainingServerMessage::Error(format!("invalid message: {e}")),
                                )
                                .await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    }
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    message: &TrainingServerMessage,
) -> Result<()> {
    let text = serde_json::to_string(message).unwrap_or_default();
    socket
        .send(axum::extract::ws::Message::Text(text.into()))
        .await
        .map_err(|e| Error::Internal(e.to_string()))
}
