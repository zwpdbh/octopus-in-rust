//! WebSocket simulation route.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use faf_sim_protocol::{SimClientMessage, SimEvent, SimServerMessage};
use faf_sim_service::SimulationService;

use crate::state::AppState;

/// Upgrade an HTTP connection to a WebSocket and run the simulation.
pub async fn simulate_ws_handler(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket) {
    use axum::extract::ws::Message;

    // Wait for the client to send the plan to simulate.
    let start_msg = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<SimClientMessage>(&text) {
                    Ok(SimClientMessage::StartPlan { plan, speed }) => {
                        break (plan, speed);
                    }
                    Ok(SimClientMessage::Command(_)) => {
                        let _ = send_json(
                            &mut socket,
                            &SimServerMessage::Error(
                                "expected StartPlan before commands".to_string(),
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &SimServerMessage::Error(format!("invalid message: {e}")),
                        )
                        .await;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue,
        }
    };

    let (plan, speed) = start_msg;

    // Start the simulation. It runs in a background thread and communicates
    // through the controller's channels.
    let service = SimulationService::new();
    let controller = service.run(plan, speed);
    let cmd_tx = controller.cmd_tx;
    let event_rx = controller.event_rx;

    // Bridge the synchronous simulation event channel into the async task.
    let (event_tx, mut event_rx_async) = tokio::sync::mpsc::unbounded_channel::<SimEvent>();
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
                        if send_json(&mut socket, &SimServerMessage::Event(event))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => {
                        // Simulation thread dropped the sender; queue is done.
                        finished = true;
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SimClientMessage>(&text) {
                            Ok(SimClientMessage::Command(cmd)) => {
                                let _ = cmd_tx.send(cmd);
                            }
                            Ok(SimClientMessage::StartPlan { .. }) => {
                                let _ = send_json(
                                    &mut socket,
                                    &SimServerMessage::Error("already started".to_string()),
                                )
                                .await;
                            }
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &SimServerMessage::Error(format!("invalid message: {e}")),
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
        let _ = send_json(&mut socket, &SimServerMessage::Finished).await;
    }
    let _ = socket.close().await;
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    message: &SimServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).unwrap_or_default();
    socket.send(axum::extract::ws::Message::Text(text)).await
}
