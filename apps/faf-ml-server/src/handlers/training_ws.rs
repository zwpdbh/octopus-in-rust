//! WebSocket training route (`GET /ws/training`), modeled on fafcn's
//! `/ws/simulate`: the client starts a run with `Start { config, speed }`,
//! the server streams `TrainingServerMessage`s from the training thread and
//! forwards `Command` frames back into it.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use faf_ml_core::{TrainingClientMessage, TrainingServerMessage};

use crate::{error::Result, state::AppState, training_service::TrainingService};

/// Upgrade an HTTP connection to a WebSocket and run a training job.
pub async fn training_ws_handler(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket) {
    use axum::extract::ws::Message;

    // Wait for the client to send the training config.
    let (config, speed) = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<TrainingClientMessage>(&text) {
                    Ok(TrainingClientMessage::Start { config, speed }) => break (config, speed),
                    Ok(TrainingClientMessage::Command(_)) => {
                        let _ = send_json(
                            &mut socket,
                            &TrainingServerMessage::Error(
                                "expected Start before commands".to_string(),
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

    // Start the training thread; communicate through its channels.
    let controller = TrainingService::run(config, speed);
    let cmd_tx = controller.cmd_tx;
    let event_rx = controller.event_rx;

    // Bridge the synchronous training event channel into the async task.
    let (event_tx, mut event_rx_async) =
        tokio::sync::mpsc::unbounded_channel::<TrainingServerMessage>();
    tokio::task::spawn_blocking(move || {
        while let Ok(event) = event_rx.recv() {
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    // Stream events to the client and read runtime commands in the same task.
    let mut finished = false;
    loop {
        tokio::select! {
            event = event_rx_async.recv() => {
                match event {
                    Some(event) => {
                        if send_json(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        // Training thread dropped the sender; run is over.
                        finished = true;
                        break;
                    }
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
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &TrainingServerMessage::Error(format!("invalid message: {e}")),
                                )
                                .await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    if finished {
        let _ = send_json(&mut socket, &TrainingServerMessage::Finished).await;
    }
    // Dropping the socket closes it (axum 0.8 has no explicit close()).
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    message: &TrainingServerMessage,
) -> Result<()> {
    let text = serde_json::to_string(message).unwrap_or_default();
    socket
        .send(axum::extract::ws::Message::Text(text.into()))
        .await
        .map_err(|e| crate::error::Error::Internal(e.to_string()))
}
