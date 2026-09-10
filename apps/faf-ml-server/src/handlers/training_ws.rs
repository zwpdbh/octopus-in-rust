//! Training routes: `GET /ws/training` (start/attach + live stream) and
//! `GET /api/training/status` (the run registry as JSON).
//!
//! The registry (`AppState::training_run`) owns the run; this handler is just
//! a viewer: it replays buffered metrics, forwards live broadcast events, and
//! relays commands. A client disconnect ends only the forwarding — training
//! always continues server-side.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    Json,
};
use faf_ml_core::{
    TrainingClientMessage, TrainingRunStatus, TrainingServerMessage, TrainingStatus,
};
use tokio::sync::broadcast;

use crate::{
    error::{Error, Result},
    state::AppState,
    training_service::TrainingService,
};

/// `GET /api/training/status` — the current/last run (404 before the first).
pub async fn get_training_status(State(state): State<AppState>) -> Result<Json<TrainingRunStatus>> {
    let guard = state
        .training_run
        .lock()
        .expect("training registry poisoned");
    let run = guard.as_ref().ok_or(Error::NotFound)?;
    Ok(Json(TrainingRunStatus {
        config: run.config.clone(),
        started_at: run.started_at,
        status: run.status.clone(),
        points: run.points.len(),
        latest: run.points.last().cloned(),
        result: run.result.clone(),
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

    // First frame decides: start a new run or attach to the active one.
    let started = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<TrainingClientMessage>(&text) {
                    Ok(TrainingClientMessage::Start { config, speed }) => {
                        match TrainingService::run(&state, config, speed) {
                            Ok(()) => break true,
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &TrainingServerMessage::Error(format!("{e:#}")),
                                )
                                .await;
                                return;
                            }
                        }
                    }
                    Ok(TrainingClientMessage::Attach) => {
                        let active = state
                            .training_run
                            .lock()
                            .expect("training registry poisoned")
                            .as_ref()
                            .is_some_and(|r| r.result.is_none());
                        if active {
                            break false;
                        }
                        let _ = send_json(
                            &mut socket,
                            &TrainingServerMessage::Error("no active training run".to_string()),
                        )
                        .await;
                        return;
                    }
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
    };
    let _ = started; // (replay covers both paths identically)

    // Snapshot the replay (points + status) and subscribe to live events
    // under the same lock so no event interleaves between the two.
    let (replay, status, cmd_tx, mut events_rx) = {
        let guard = state
            .training_run
            .lock()
            .expect("training registry poisoned");
        match guard.as_ref() {
            Some(run) => (
                run.points.clone(),
                run.status.clone(),
                run.cmd_tx.clone(),
                run.events_tx.subscribe(),
            ),
            None => return, // run vanished (shouldn't happen right after Start/Attach)
        }
    };
    for point in replay {
        if send_json(&mut socket, &TrainingServerMessage::Metrics(point))
            .await
            .is_err()
        {
            return;
        }
    }
    if send_json(&mut socket, &TrainingServerMessage::Status(status))
        .await
        .is_err()
    {
        return;
    }

    // Forward live events + relay commands until the client leaves or the
    // run reaches a terminal status.
    loop {
        tokio::select! {
            event = events_rx.recv() => {
                match event {
                    Ok(msg) => {
                        let terminal = matches!(
                            &msg,
                            TrainingServerMessage::Status(
                                TrainingStatus::Done { .. } | TrainingStatus::Failed { .. }
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
                                let _ = cmd_tx.send(cmd);
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
