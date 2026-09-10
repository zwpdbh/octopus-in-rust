//! Training-run manager: owns the `/ws/training` WebSocket internally so
//! MCP callers only see plain tool calls (`training_start` → handle,
//! `training_status`, `training_command`).

use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context};
use faf_ml_core::{
    TrainingClientMessage, TrainingCommand, TrainingConfig, TrainingMetricsPoint,
    TrainingServerMessage,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};

/// Live state of one training run, updated by the socket reader task.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RunState {
    pub status: String,
    pub detail: String,
    /// Metrics points received so far.
    pub points: usize,
    /// The most recent metrics point (progress + current losses).
    pub latest: Option<TrainingMetricsPoint>,
}

struct RunHandle {
    state: Arc<Mutex<RunState>>,
    cmd_tx: mpsc::UnboundedSender<TrainingCommand>,
}

/// Registry of training runs started through this MCP server (in-memory;
/// runs do not survive a server restart).
#[derive(Clone, Default)]
pub struct TrainingManager {
    runs: Arc<Mutex<HashMap<String, RunHandle>>>,
    last_started: Arc<Mutex<Option<String>>>,
}

impl TrainingManager {
    /// Open `/ws/training`, send `Start { config, speed }`, and spawn the
    /// reader task. Returns the run handle (a uuid string).
    pub async fn start(
        &self,
        api_base: &str,
        config: TrainingConfig,
        speed: f64,
    ) -> anyhow::Result<String> {
        let ws_base = match api_base.strip_prefix("https") {
            Some(rest) => format!("wss{rest}"),
            None => api_base.replacen("http", "ws", 1),
        };
        let url = format!("{ws_base}/ws/training");
        let (socket, _) = tokio_tungstenite::connect_async(&url)
            .await
            .with_context(|| format!("connecting to {url}"))?;
        let (mut sink, mut stream) = socket.split();

        let start = serde_json::to_string(&TrainingClientMessage::Start { config, speed })?;
        sink.send(tokio_tungstenite::tungstenite::Message::Text(start.into()))
            .await?;

        let handle = uuid::Uuid::new_v4().to_string();
        let state = Arc::new(Mutex::new(RunState::default()));
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TrainingCommand>();

        let task_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    frame = stream.next() => {
                        match frame {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                match serde_json::from_str::<TrainingServerMessage>(&text) {
                                    Ok(TrainingServerMessage::Metrics(point)) => {
                                        let mut s = task_state.lock().await;
                                        s.points += 1;
                                        s.latest = Some(point);
                                    }
                                    Ok(TrainingServerMessage::Status(status)) => {
                                        let mut s = task_state.lock().await;
                                        s.status = match &status {
                                            faf_ml_core::TrainingStatus::Running => "running".into(),
                                            faf_ml_core::TrainingStatus::Paused => "paused".into(),
                                            faf_ml_core::TrainingStatus::Done { duration_secs } => {
                                                s.detail = format!("done in {duration_secs}s");
                                                "done".into()
                                            }
                                            faf_ml_core::TrainingStatus::Failed { error } => {
                                                s.detail = error.clone();
                                                "failed".into()
                                            }
                                        };
                                    }
                                    Ok(TrainingServerMessage::Error(err)) => {
                                        task_state.lock().await.detail = format!("error: {err}");
                                    }
                                    Ok(TrainingServerMessage::Finished) => break,
                                    Err(err) => {
                                        task_state.lock().await.detail =
                                            format!("parse error: {err}");
                                    }
                                }
                            }
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                            Some(Ok(_)) => {}
                            Some(Err(err)) => {
                                task_state.lock().await.detail = format!("socket error: {err}");
                                break;
                            }
                        }
                    }
                    cmd = cmd_rx.recv() => {
                        let Some(cmd) = cmd else { break };
                        let text = serde_json::to_string(&TrainingClientMessage::Command(cmd))
                            .unwrap_or_default();
                        if sink
                            .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        self.runs
            .lock()
            .await
            .insert(handle.clone(), RunHandle { state, cmd_tx });
        *self.last_started.lock().await = Some(handle.clone());
        Ok(handle)
    }

    async fn resolve(&self, handle: Option<&str>) -> anyhow::Result<String> {
        match handle {
            Some(h) => Ok(h.to_string()),
            None => self
                .last_started
                .lock()
                .await
                .clone()
                .ok_or_else(|| anyhow!("no training runs yet in this MCP server session")),
        }
    }

    /// Snapshot of one run's state (most recent when `handle` is omitted).
    pub async fn status(&self, handle: Option<&str>) -> anyhow::Result<RunState> {
        let handle = self.resolve(handle).await?;
        let state = {
            let runs = self.runs.lock().await;
            let run = runs
                .get(&handle)
                .ok_or_else(|| anyhow!("unknown training run {handle}"))?;
            run.state.clone()
        };
        let snapshot = state.lock().await.clone();
        Ok(snapshot)
    }

    /// Forward a command to a running job.
    pub async fn command(&self, handle: &str, cmd: TrainingCommand) -> anyhow::Result<()> {
        let runs = self.runs.lock().await;
        let run = runs
            .get(handle)
            .ok_or_else(|| anyhow!("unknown training run {handle}"))?;
        run.cmd_tx
            .send(cmd)
            .map_err(|_| anyhow!("training run {handle} is no longer connected"))
    }
}
