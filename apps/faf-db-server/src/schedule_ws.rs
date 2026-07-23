//! WebSocket endpoint for live scheduling with cancellation support.
//!
//! The client sends a `Start` message with the same payload as `POST /api/schedule`,
//! then the server streams one message per committed step and finally sends either
//! `Done` with the full result or `Error` if scheduling failed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use faf_build_scheduler::{ScheduleError, ScheduleStreamEvent, Scheduler};
use faf_sim_shared::{Schedule, ScheduleWithReasoning, StepReasoning};
use serde::{Deserialize, Serialize};

use crate::schedule_api::ScheduleApiRequest;

/// Messages sent by the client over the scheduling WebSocket.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ScheduleClientMessage {
    #[serde(rename = "start")]
    Start { request: ScheduleApiRequest },
    #[serde(rename = "cancel")]
    Cancel,
}

/// Messages sent by the server over the scheduling WebSocket.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ScheduleServerMessage {
    #[serde(rename = "step")]
    Step {
        step_number: usize,
        step: faf_sim_shared::StepResult,
        reasoning: StepReasoning,
    },
    #[serde(rename = "done")]
    Done {
        schedule: Schedule,
        reasoning: Vec<StepReasoning>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Handle a scheduling WebSocket connection.
pub async fn handle_schedule_socket(scheduler: Arc<Scheduler>, mut socket: WebSocket) {
    // Wait for the client to start a scheduling run.
    let request = match wait_for_start(&mut socket).await {
        Some(req) => req,
        None => return,
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let (event_tx, event_rx) = std::sync::mpsc::channel::<ScheduleStreamEvent>();

    // Run the scheduler in a blocking task so the async runtime is not blocked.
    let scheduler_cancelled = Arc::clone(&cancelled);
    let mut scheduler_handle = tokio::task::spawn_blocking(move || match request {
        ScheduleApiRequest::Eco {
            initial_eco,
            initial_inventory,
            target_mass_production,
            tolerance,
            options,
            max_mex_count,
        } => {
            use faf_build_scheduler::{EcoScheduleRequest, EcoTarget, SchedulerConfig};
            use faf_quantities::MassRate;
            scheduler.schedule_eco_stream(
                &EcoScheduleRequest {
                    initial_eco,
                    initial_inventory,
                    target: EcoTarget {
                        mass_production: MassRate::from_raw(target_mass_production),
                        tolerance,
                    },
                    options,
                    config: SchedulerConfig { max_mex_count },
                },
                event_tx,
                scheduler_cancelled,
            )
        }
        ScheduleApiRequest::Unit {
            initial_eco,
            initial_inventory,
            target,
            options,
            max_mex_count,
        } => {
            use faf_build_scheduler::{SchedulerConfig, UnitScheduleRequest};
            scheduler.schedule_unit_stream(
                &UnitScheduleRequest {
                    initial_eco,
                    initial_inventory,
                    target,
                    options,
                    config: SchedulerConfig { max_mex_count },
                },
                event_tx,
                scheduler_cancelled,
            )
        }
    });

    // Bridge the synchronous scheduler event channel into an async channel.
    let (async_tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel::<ScheduleStreamEvent>();
    let bridge_cancelled = Arc::clone(&cancelled);
    let _bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = event_rx.recv() {
            if async_tx.send(event).is_err() {
                bridge_cancelled.store(true, Ordering::Relaxed);
                break;
            }
        }
    });

    let mut step_number: usize = 0;
    let mut result: Option<Result<ScheduleWithReasoning, ScheduleError>> = None;

    loop {
        tokio::select! {
            Some(event) = async_rx.recv() => {
                match event {
                    ScheduleStreamEvent { step, reasoning, .. } => {
                        step_number += 1;
                        let msg = ScheduleServerMessage::Step {
                            step_number,
                            step: step.clone(),
                            reasoning: reasoning.clone(),
                        };
                        if send_message(&mut socket, msg).await.is_err() {
                            cancelled.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
            scheduler_result = &mut scheduler_handle => {
                result = Some(scheduler_result.unwrap_or_else(|_| {
                    Err(ScheduleError::AlgorithmNotImplemented("scheduler task panicked".to_string()))
                }));
                break;
            }
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<ScheduleClientMessage>(&text) {
                            Ok(ScheduleClientMessage::Cancel) => {
                                cancelled.store(true, Ordering::Relaxed);
                            }
                            Ok(ScheduleClientMessage::Start { .. }) => {
                                let _ = send_message(
                                    &mut socket,
                                    ScheduleServerMessage::Error {
                                        message: "already started".to_string(),
                                    },
                                ).await;
                            }
                            Err(e) => {
                                let _ = send_message(
                                    &mut socket,
                                    ScheduleServerMessage::Error {
                                        message: format!("invalid message: {e}"),
                                    },
                                ).await;
                            }
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => {
                        cancelled.store(true, Ordering::Relaxed);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Drain any remaining events without sending them, then send the final result.
    while async_rx.recv().await.is_some() {}

    if let Some(result) = result {
        match result {
            Ok(payload) => {
                let _ = send_message(
                    &mut socket,
                    ScheduleServerMessage::Done {
                        schedule: payload.schedule,
                        reasoning: payload.reasoning,
                    },
                )
                .await;
            }
            Err(ScheduleError::Cancelled) => {
                // The connection is about to close; no need to send a special
                // cancelled message unless the client wants one.
            }
            Err(e) => {
                let _ = send_message(
                    &mut socket,
                    ScheduleServerMessage::Error {
                        message: e.to_string(),
                    },
                )
                .await;
            }
        }
    }

    let _ = socket.close().await;
}

async fn wait_for_start(socket: &mut WebSocket) -> Option<ScheduleApiRequest> {
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<ScheduleClientMessage>(&text) {
                    Ok(ScheduleClientMessage::Start { request }) => return Some(request),
                    Ok(ScheduleClientMessage::Cancel) => {
                        // Nothing to cancel yet; keep waiting.
                        continue;
                    }
                    Err(e) => {
                        let _ = send_message(
                            socket,
                            ScheduleServerMessage::Error {
                                message: format!("invalid message: {e}"),
                            },
                        )
                        .await;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return None,
            Some(Err(_)) => return None,
            _ => {}
        }
    }
}

async fn send_message(
    socket: &mut WebSocket,
    msg: ScheduleServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
    socket.send(Message::Text(text)).await
}
