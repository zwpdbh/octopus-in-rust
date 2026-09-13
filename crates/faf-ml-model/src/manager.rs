//! `TrainManager`: training as a long-running async service.
//!
//! One tokio task (the actor) owns the run state and is its sole mutator.
//! Callers hold a [`TrainManagerHandle`] and talk to it over a mailbox of
//! [`TrainCmd`]s (every command carries a oneshot reply); viewers subscribe
//! to the [`ManagerEvent`] broadcast. The burn training loop is sync, so it
//! stays on a `std::thread`: control flows in via a `watch` channel (read at
//! batch boundaries), events flow out over an unbounded channel.
//!
//! Late events from a winding-down (reset) run are ignored via a
//! **generation counter**: each run gets an id and the manager only accepts
//! messages tagged with the current one.
//!
//! ```text
//! web UI / MCP / REST  ──TrainCmd over mpsc (oneshot replies)──▶ TrainManager
//!                                                                   │  watch<ControlState> in
//!                                                              std::thread running train()
//!                                                                   │  (generation, ThreadMsg) out
//! ```

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::train::{train, ControlState, TrainAction, TrainEvent, TrainExit, TrainParams};
use crate::{AdB, CpuAdB};

/// Runtime command accepted by the manager (the model-level counterpart of
/// the wire `TrainingCommand`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ManagerCommand {
    Pause,
    Resume,
    /// Finish the current batch, save the checkpoint, end the run (the run
    /// record stays visible as `Ended`).
    Stop,
    /// Like Stop (checkpoint still saved), but additionally wipes the run
    /// record, broadcasts `Reset` so viewers clear charts, and returns to
    /// idle immediately — a new Start is accepted while the old thread
    /// finishes its last batch + save.
    Reset,
    SetSpeed {
        batches_per_sec: f64,
    },
}

/// Phase of an active run. `Pausing`/`Stopping` mean the command was
/// accepted but the training thread has not reached the batch boundary yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    Pausing,
    Paused,
    Stopping,
}

/// Terminal outcome of a run.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Completed {
        run_dir: PathBuf,
        duration_secs: u64,
    },
    /// Ended via Stop OR Reset — the checkpoint was saved either way.
    Stopped {
        run_dir: PathBuf,
        duration_secs: u64,
    },
    Failed {
        error: String,
    },
}

/// Lifecycle of the (single) training run slot.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStatus {
    Idle,
    Active {
        phase: Phase,
        config: TrainParams,
        started_at: DateTime<Utc>,
    },
    Ended {
        outcome: Outcome,
        config: TrainParams,
        started_at: DateTime<Utc>,
    },
}

/// Event broadcast to all viewers.
#[derive(Debug, Clone, PartialEq)]
pub enum ManagerEvent {
    /// A training metrics event (Batch / EpochEnd).
    Train(TrainEvent),
    /// Instant control acknowledgment (Pausing/Stopping) or settled phase
    /// (Running/Paused).
    PhaseChanged(Phase),
    /// Terminal outcome of the run.
    Ended(Outcome),
    /// The run record was wiped — viewers clear their charts.
    Reset,
}

/// Atomic snapshot for late/reattaching viewers: run status, replay buffer,
/// and a live subscription, all taken under one actor turn so no event can
/// interleave between them.
pub struct AttachDump {
    pub status: RunStatus,
    /// Buffered Batch/EpochEnd events (replay for new viewers). Batch points
    /// are capped at [`MAX_REPLAY_BATCH_EVENTS`] (oldest dropped); epoch
    /// summaries are kept for the whole run.
    pub replay: Vec<TrainEvent>,
    pub events: broadcast::Receiver<ManagerEvent>,
}

/// Per-batch points retained in the replay buffer. Epoch summaries never
/// count against the cap — a long run's memory stays bounded while the
/// epoch-level curve remains complete.
const MAX_REPLAY_BATCH_EVENTS: usize = 5000;

/// Point-in-time snapshot for `GET /api/training/status`.
pub struct Snapshot {
    pub status: RunStatus,
    pub replay: Vec<TrainEvent>,
}

/// Message from a training thread to the manager (generation-tagged so a
/// winding-down run's late messages are dropped after Reset).
enum ThreadMsg {
    Event(TrainEvent),
    /// String (not anyhow::Error) so the message stays `Send` + simple.
    Exited(Result<TrainExit, String>),
}

/// Mailbox message to the manager actor (all carry a oneshot reply).
enum TrainCmd {
    Start {
        params: TrainParams,
        speed: f64,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Command {
        cmd: ManagerCommand,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Attach {
        reply: oneshot::Sender<AttachDump>,
    },
    Snapshot {
        reply: oneshot::Sender<Snapshot>,
    },
}

/// Spawns a training backend for one run. The default spawns the real burn
/// thread; tests inject a fake driving the same channels.
type TrainerFactory = Arc<
    dyn Fn(TrainParams, watch::Receiver<ControlState>, mpsc::UnboundedSender<(u64, ThreadMsg)>, u64)
        + Send
        + Sync,
>;

/// Cloneable handle to the manager actor.
#[derive(Clone)]
pub struct TrainManagerHandle {
    cmd_tx: mpsc::Sender<TrainCmd>,
}

impl TrainManagerHandle {
    /// Spawn the manager actor on the caller's tokio runtime.
    pub fn spawn() -> Self {
        Self::spawn_with(Arc::new(spawn_train_thread))
    }

    fn spawn_with(trainer: TrainerFactory) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (thread_tx, thread_rx) = mpsc::unbounded_channel();
        let (events_tx, _) = broadcast::channel(1024);
        let manager = TrainManager {
            run: RunStatus::Idle,
            generation: 0,
            replay: VecDeque::new(),
            replay_batches: 0,
            events_tx,
            control_tx: None,
            trainer,
            cmd_rx,
            thread_tx,
            thread_rx,
        };
        tokio::spawn(manager.run());
        Self { cmd_tx }
    }

    /// Start a run. Busy/dataset validation failures come back in the reply.
    pub async fn start(&self, params: TrainParams, speed: f64) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(TrainCmd::Start {
                params,
                speed,
                reply,
            })
            .await
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    /// Send a runtime command (Pause/Resume/Stop/Reset/SetSpeed).
    pub async fn command(&self, cmd: ManagerCommand) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(TrainCmd::Command { cmd, reply })
            .await
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    /// Atomic replay + live subscription for a (re)attaching viewer.
    pub async fn attach(&self) -> Result<AttachDump, String> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(TrainCmd::Attach { reply })
            .await
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())
    }

    /// Point-in-time status (for the REST status endpoint).
    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(TrainCmd::Snapshot { reply })
            .await
            .map_err(|_| gone())?;
        rx.await.map_err(|_| gone())
    }
}

fn gone() -> String {
    "training manager is gone".to_string()
}

/// Spawn the real training thread: `train()` on one thread, plus a tiny
/// forwarder thread that tags events with the run generation. The forwarder
/// is joined before `Exited` is sent so the exit can never overtake metrics.
fn spawn_train_thread(
    params: TrainParams,
    control: watch::Receiver<ControlState>,
    events: mpsc::UnboundedSender<(u64, ThreadMsg)>,
    generation: u64,
) {
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<TrainEvent>();
    let fwd_tx = events.clone();
    let forwarder = std::thread::spawn(move || {
        while let Some(event) = ev_rx.blocking_recv() {
            if fwd_tx.send((generation, ThreadMsg::Event(event))).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        let result = if params.cpu {
            train::<CpuAdB>(&params, ev_tx, control)
        } else {
            train::<AdB>(&params, ev_tx, control)
        };
        let exit = result.map_err(|e| format!("{e:#}"));
        // `train` dropped `ev_tx` on return, so the forwarder has flushed
        // every event once it exits.
        let _ = forwarder.join();
        let _ = events.send((generation, ThreadMsg::Exited(exit)));
    });
}

/// The manager actor: single `select!` loop, sole mutator of run state.
struct TrainManager {
    run: RunStatus,
    /// Id of the current run; bumped on Start and Reset.
    generation: u64,
    /// Buffered Batch/EpochEnd events (replayed to new viewers; survives
    /// into `Ended` so late attachers still see the last run). Batch points
    /// are capped (see [`MAX_REPLAY_BATCH_EVENTS`]); epoch ends are kept.
    replay: VecDeque<TrainEvent>,
    /// Number of `Batch` events currently in `replay` (epoch ends excluded).
    replay_batches: usize,
    events_tx: broadcast::Sender<ManagerEvent>,
    /// Control channel of the active run (`None` when idle/ended).
    control_tx: Option<watch::Sender<ControlState>>,
    trainer: TrainerFactory,
    cmd_rx: mpsc::Receiver<TrainCmd>,
    thread_tx: mpsc::UnboundedSender<(u64, ThreadMsg)>,
    thread_rx: mpsc::UnboundedReceiver<(u64, ThreadMsg)>,
}

impl TrainManager {
    async fn run(mut self) {
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => self.handle_cmd(cmd),
                        None => break,
                    }
                }
                msg = self.thread_rx.recv() => {
                    if let Some((generation, msg)) = msg {
                        self.handle_thread_msg(generation, msg);
                    }
                }
            }
        }
    }

    fn handle_cmd(&mut self, cmd: TrainCmd) {
        match cmd {
            TrainCmd::Start {
                params,
                speed,
                reply,
            } => {
                let _ = reply.send(self.start(params, speed));
            }
            TrainCmd::Command { cmd, reply } => {
                let _ = reply.send(self.command(cmd));
            }
            TrainCmd::Attach { reply } => {
                let _ = reply.send(AttachDump {
                    status: self.run.clone(),
                    replay: self.replay.iter().cloned().collect(),
                    events: self.events_tx.subscribe(),
                });
            }
            TrainCmd::Snapshot { reply } => {
                let _ = reply.send(Snapshot {
                    status: self.run.clone(),
                    replay: self.replay.iter().cloned().collect(),
                });
            }
        }
    }

    fn start(&mut self, params: TrainParams, speed: f64) -> Result<(), String> {
        if matches!(self.run, RunStatus::Active { .. }) {
            return Err("a training run is already active".to_string());
        }
        validate_dataset(&params)?;
        self.generation += 1;
        let (control_tx, control_rx) = watch::channel(ControlState {
            action: TrainAction::Continue,
            batches_per_sec: speed,
        });
        (self.trainer)(
            params.clone(),
            control_rx,
            self.thread_tx.clone(),
            self.generation,
        );
        self.control_tx = Some(control_tx);
        self.replay.clear();
        self.replay_batches = 0;
        self.run = RunStatus::Active {
            phase: Phase::Running,
            config: params,
            started_at: Utc::now(),
        };
        self.broadcast(ManagerEvent::PhaseChanged(Phase::Running));
        Ok(())
    }

    fn command(&mut self, cmd: ManagerCommand) -> Result<(), String> {
        match cmd {
            ManagerCommand::Reset => {
                // Checkpoint still saved: the thread sees Abort and exits
                // through the normal finish path; its late messages are
                // dropped by the generation bump.
                self.set_action(TrainAction::Abort);
                self.generation += 1;
                self.control_tx = None;
                self.replay.clear();
                self.replay_batches = 0;
                self.run = RunStatus::Idle;
                self.broadcast(ManagerEvent::Reset);
                Ok(())
            }
            ManagerCommand::Pause => {
                let phase = self.active_phase()?;
                if matches!(phase, Phase::Running) {
                    self.set_action(TrainAction::Pause);
                    self.set_phase(Phase::Pausing);
                }
                Ok(())
            }
            ManagerCommand::Resume => {
                let phase = self.active_phase()?;
                if matches!(phase, Phase::Pausing | Phase::Paused) {
                    self.set_action(TrainAction::Continue);
                    self.set_phase(Phase::Running);
                }
                Ok(())
            }
            ManagerCommand::Stop => {
                let phase = self.active_phase()?;
                if !matches!(phase, Phase::Stopping) {
                    self.set_action(TrainAction::Abort);
                    self.set_phase(Phase::Stopping);
                }
                Ok(())
            }
            ManagerCommand::SetSpeed { batches_per_sec } => {
                self.active_phase()?;
                if let Some(tx) = &self.control_tx {
                    tx.send_modify(|c| c.batches_per_sec = batches_per_sec);
                }
                Ok(())
            }
        }
    }

    fn handle_thread_msg(&mut self, generation: u64, msg: ThreadMsg) {
        if generation != self.generation {
            return; // winding-down run after Reset
        }
        match msg {
            ThreadMsg::Event(event) => {
                if matches!(event, TrainEvent::Paused) {
                    // A stale Paused arriving while Running (thread entered
                    // the hold just as Resume landed) is ignored.
                    if matches!(
                        &self.run,
                        RunStatus::Active {
                            phase: Phase::Pausing,
                            ..
                        }
                    ) {
                        self.set_phase(Phase::Paused);
                    }
                    return;
                }
                self.push_replay(event.clone());
                self.broadcast(ManagerEvent::Train(event));
            }
            ThreadMsg::Exited(result) => {
                let outcome = match result {
                    Ok(TrainExit::Completed {
                        run_dir,
                        duration_secs,
                    }) => Outcome::Completed {
                        run_dir,
                        duration_secs,
                    },
                    Ok(TrainExit::Aborted {
                        run_dir,
                        duration_secs,
                    }) => Outcome::Stopped {
                        run_dir,
                        duration_secs,
                    },
                    Err(error) => Outcome::Failed { error },
                };
                self.control_tx = None;
                if let RunStatus::Active {
                    config, started_at, ..
                } = std::mem::replace(&mut self.run, RunStatus::Idle)
                {
                    self.run = RunStatus::Ended {
                        outcome: outcome.clone(),
                        config,
                        started_at,
                    };
                    self.broadcast(ManagerEvent::Ended(outcome));
                }
            }
        }
    }

    fn active_phase(&self) -> Result<Phase, String> {
        match &self.run {
            RunStatus::Active { phase, .. } => Ok(*phase),
            _ => Err("no active run".to_string()),
        }
    }

    /// Buffer an event for late attachers. Epoch summaries are kept for the
    /// whole run; batch points beyond [`MAX_REPLAY_BATCH_EVENTS`] drop the
    /// oldest first, so a long run can't grow memory without bound.
    fn push_replay(&mut self, event: TrainEvent) {
        let is_batch = matches!(event, TrainEvent::Batch { .. });
        self.replay.push_back(event);
        if !is_batch {
            return;
        }
        self.replay_batches += 1;
        if self.replay_batches > MAX_REPLAY_BATCH_EVENTS {
            let oldest_batch = self
                .replay
                .iter()
                .position(|e| matches!(e, TrainEvent::Batch { .. }));
            if let Some(pos) = oldest_batch {
                self.replay.remove(pos);
                self.replay_batches -= 1;
            }
        }
    }

    fn set_action(&self, action: TrainAction) {
        if let Some(tx) = &self.control_tx {
            tx.send_modify(|c| c.action = action);
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        if let RunStatus::Active { phase: p, .. } = &mut self.run {
            *p = phase;
        }
        self.broadcast(ManagerEvent::PhaseChanged(phase));
    }

    fn broadcast(&self, event: ManagerEvent) {
        let _ = self.events_tx.send(event);
    }
}

/// Snapshot pre-flight check, run synchronously in `start` so a bad snapshot
/// fails the Start request itself instead of surfacing as a `Failed` run
/// seconds later. Checks the name, and that the manifest exists, parses, and
/// has enough samples (params carry `data`/`out` paths, so the manager stays
/// path-agnostic; snapshots live in `datasets/` under the store root by
/// `TrainParams` convention). Image/label integrity is still verified by
/// `DetectDataset::load_snapshot` on the training thread.
fn validate_dataset(params: &TrainParams) -> Result<(), String> {
    let name = params.dataset.trim();
    if name.is_empty() {
        return Err(
            "no dataset snapshot selected — create one on the Datasets page first".to_string(),
        );
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!(
            "invalid dataset name {name:?}: only [A-Za-z0-9._-] allowed"
        ));
    }
    let snapshot = params
        .data
        .join("datasets")
        .join(format!("{}.json", params.dataset));
    if !snapshot.is_file() {
        return Err(format!(
            "snapshot {:?} not found — create one on the Datasets page first",
            params.dataset
        ));
    }
    let raw = std::fs::read_to_string(&snapshot)
        .map_err(|e| format!("reading {}: {e}", snapshot.display()))?;
    let manifest: faf_ml_core::DatasetManifest =
        serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", snapshot.display()))?;
    if manifest.entries.len() < 2 {
        return Err(format!(
            "snapshot {:?} has {} sample(s) — need at least 2 to train",
            params.dataset,
            manifest.entries.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A store dir with a `datasets/test.json` snapshot in it (valid
    /// manifest, 2 entries — passes the start pre-flight).
    fn test_params() -> (PathBuf, TrainParams) {
        let dir = std::env::temp_dir().join(format!("train-manager-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("datasets")).unwrap();
        let manifest = serde_json::json!({
            "name": "test",
            "created_at": "2026-01-01T00:00:00Z",
            "entries": [
                {"image_id": uuid::Uuid::new_v4(), "labels": []},
                {"image_id": uuid::Uuid::new_v4(), "labels": []},
            ],
        });
        std::fs::write(dir.join("datasets/test.json"), manifest.to_string()).unwrap();
        let params = TrainParams {
            data: dir.clone(),
            dataset: "test".to_string(),
            ..Default::default()
        };
        (dir, params)
    }

    /// Fake trainer: emit one batch point, then complete immediately.
    fn completes() -> TrainerFactory {
        Arc::new(|_params, _control, events, generation| {
            tokio::spawn(async move {
                let _ = events.send((
                    generation,
                    ThreadMsg::Event(TrainEvent::Batch {
                        epoch: 1,
                        batch: 1,
                        total_batches: 1,
                        cls_loss: 1.0,
                        bbox_loss: 1.0,
                        total_loss: 2.0,
                    }),
                ));
                let _ = events.send((
                    generation,
                    ThreadMsg::Exited(Ok(TrainExit::Completed {
                        run_dir: PathBuf::from("runs/20260912-000000"),
                        duration_secs: 1,
                    })),
                ));
            });
        })
    }

    /// Fake trainer honoring the control channel: holds until Pause (then
    /// confirms with `TrainEvent::Paused`), exits Aborted on Abort.
    fn controllable() -> TrainerFactory {
        Arc::new(|_params, mut control, events, generation| {
            tokio::spawn(async move {
                let mut paused_sent = false;
                loop {
                    let action = control.borrow().action;
                    match action {
                        TrainAction::Continue => {}
                        TrainAction::Pause => {
                            if !paused_sent {
                                paused_sent = true;
                                let _ =
                                    events.send((generation, ThreadMsg::Event(TrainEvent::Paused)));
                            }
                        }
                        TrainAction::Abort => {
                            let _ = events.send((
                                generation,
                                ThreadMsg::Exited(Ok(TrainExit::Aborted {
                                    run_dir: PathBuf::from("runs/20260912-000000"),
                                    duration_secs: 1,
                                })),
                            ));
                            return;
                        }
                    }
                    if control.changed().await.is_err() {
                        return;
                    }
                }
            });
        })
    }

    /// Fake trainer that fails.
    fn fails() -> TrainerFactory {
        Arc::new(|_params, _control, events, generation| {
            tokio::spawn(async move {
                let _ = events.send((generation, ThreadMsg::Exited(Err("boom".to_string()))));
            });
        })
    }

    async fn recv(rx: &mut broadcast::Receiver<ManagerEvent>) -> ManagerEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for manager event")
            .expect("broadcast closed")
    }

    #[tokio::test]
    async fn start_rejects_second_start() {
        let handle = TrainManagerHandle::spawn_with(controllable());
        let (dir, params) = test_params();
        handle.start(params.clone(), 0.0).await.unwrap();
        let err = handle.start(params, 0.0).await.unwrap_err();
        assert!(err.contains("already active"));
        let snap = handle.snapshot().await.unwrap();
        assert!(matches!(
            snap.status,
            RunStatus::Active {
                phase: Phase::Running,
                ..
            }
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn start_validates_dataset() {
        let handle = TrainManagerHandle::spawn_with(completes());
        let err = handle.start(TrainParams::default(), 0.0).await.unwrap_err();
        assert!(err.contains("no dataset snapshot"));
        let (dir, mut params) = test_params();
        params.dataset = "missing".to_string();
        let err = handle.start(params, 0.0).await.unwrap_err();
        assert!(err.contains("not found"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn pause_then_resume() {
        let handle = TrainManagerHandle::spawn_with(controllable());
        let mut dump = handle.attach().await.unwrap();
        let (dir, params) = test_params();
        handle.start(params, 0.0).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );

        handle.command(ManagerCommand::Pause).await.unwrap();
        // Instant ack, then the settled state once the thread confirms.
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Pausing)
        );
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Paused)
        );
        let snap = handle.snapshot().await.unwrap();
        assert!(matches!(
            snap.status,
            RunStatus::Active {
                phase: Phase::Paused,
                ..
            }
        ));

        handle.command(ManagerCommand::Resume).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn stop_ends_with_stopped_outcome() {
        let handle = TrainManagerHandle::spawn_with(controllable());
        let mut dump = handle.attach().await.unwrap();
        let (dir, params) = test_params();
        handle.start(params, 0.0).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );

        handle.command(ManagerCommand::Stop).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Stopping)
        );
        let outcome = match recv(&mut dump.events).await {
            ManagerEvent::Ended(outcome) => outcome,
            other => panic!("expected Ended, got {other:?}"),
        };
        assert!(matches!(outcome, Outcome::Stopped { .. }));
        let snap = handle.snapshot().await.unwrap();
        assert!(matches!(snap.status, RunStatus::Ended { .. }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn reset_wipes_run_and_allows_new_start() {
        let handle = TrainManagerHandle::spawn_with(controllable());
        let mut dump = handle.attach().await.unwrap();
        let (dir, params) = test_params();
        handle.start(params.clone(), 0.0).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );

        handle.command(ManagerCommand::Reset).await.unwrap();
        assert_eq!(recv(&mut dump.events).await, ManagerEvent::Reset);
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.status, RunStatus::Idle);
        assert!(snap.replay.is_empty());

        // New Start accepted while the old thread winds down; its late
        // Exited is dropped (generation mismatch) and must not clobber the
        // new run.
        handle.start(params, 0.0).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = handle.snapshot().await.unwrap();
        assert!(matches!(
            snap.status,
            RunStatus::Active {
                phase: Phase::Running,
                ..
            }
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn trainer_failure_ends_failed() {
        let handle = TrainManagerHandle::spawn_with(fails());
        let mut dump = handle.attach().await.unwrap();
        let (dir, params) = test_params();
        handle.start(params, 0.0).await.unwrap();
        assert_eq!(
            recv(&mut dump.events).await,
            ManagerEvent::PhaseChanged(Phase::Running)
        );
        match recv(&mut dump.events).await {
            ManagerEvent::Ended(Outcome::Failed { error }) => assert_eq!(error, "boom"),
            other => panic!("expected Ended(Failed), got {other:?}"),
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn completed_run_replays_to_late_viewers() {
        let handle = TrainManagerHandle::spawn_with(completes());
        let (dir, params) = test_params();
        handle.start(params, 0.0).await.unwrap();
        // Wait for the run to end.
        loop {
            let snap = handle.snapshot().await.unwrap();
            if matches!(snap.status, RunStatus::Ended { .. }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let dump = handle.attach().await.unwrap();
        assert!(matches!(dump.status, RunStatus::Ended { .. }));
        assert_eq!(dump.replay.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn commands_from_idle_are_rejected() {
        let handle = TrainManagerHandle::spawn_with(controllable());
        assert!(handle.command(ManagerCommand::Pause).await.is_err());
        assert!(handle.command(ManagerCommand::Stop).await.is_err());
        // Reset from idle is a no-op Ok.
        handle.command(ManagerCommand::Reset).await.unwrap();
    }

    #[tokio::test]
    async fn start_rejects_tiny_snapshot() {
        let handle = TrainManagerHandle::spawn_with(completes());
        let (dir, params) = test_params();
        let manifest = serde_json::json!({
            "name": "test",
            "created_at": "2026-01-01T00:00:00Z",
            "entries": [{"image_id": uuid::Uuid::new_v4(), "labels": []}],
        });
        std::fs::write(dir.join("datasets/test.json"), manifest.to_string()).unwrap();
        let err = handle.start(params, 0.0).await.unwrap_err();
        assert!(err.contains("need at least 2"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn replay_caps_batch_events_but_keeps_epochs() {
        let batches_per_epoch = MAX_REPLAY_BATCH_EVENTS / 2 + 100;
        let handle = TrainManagerHandle::spawn_with(Arc::new(
            move |_params, _control, events, generation| {
                tokio::spawn(async move {
                    for epoch in 1..=2usize {
                        for batch in 1..=batches_per_epoch {
                            let _ = events.send((
                                generation,
                                ThreadMsg::Event(TrainEvent::Batch {
                                    epoch,
                                    batch,
                                    total_batches: 1,
                                    cls_loss: 1.0,
                                    bbox_loss: 1.0,
                                    total_loss: 2.0,
                                }),
                            ));
                        }
                        let _ = events.send((
                            generation,
                            ThreadMsg::Event(TrainEvent::EpochEnd {
                                epoch,
                                total_epochs: 2,
                                train_cls: 1.0,
                                train_bbox: 1.0,
                                valid_cls: 1.0,
                                valid_bbox: 1.0,
                            }),
                        ));
                    }
                    let _ = events.send((
                        generation,
                        ThreadMsg::Exited(Ok(TrainExit::Completed {
                            run_dir: PathBuf::from("runs/20260912-000000"),
                            duration_secs: 1,
                        })),
                    ));
                });
            },
        ));
        let (dir, params) = test_params();
        handle.start(params, 0.0).await.unwrap();
        loop {
            let snap = handle.snapshot().await.unwrap();
            if matches!(snap.status, RunStatus::Ended { .. }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let dump = handle.attach().await.unwrap();
        let batches = dump
            .replay
            .iter()
            .filter(|e| matches!(e, TrainEvent::Batch { .. }))
            .count();
        let epochs = dump
            .replay
            .iter()
            .filter(|e| matches!(e, TrainEvent::EpochEnd { .. }))
            .count();
        assert_eq!(batches, MAX_REPLAY_BATCH_EVENTS);
        assert_eq!(epochs, 2);
        // Oldest batch points were dropped first.
        match dump.replay.first() {
            Some(TrainEvent::Batch { epoch, batch, .. }) => {
                assert_eq!((*epoch, *batch), (1, 201));
            }
            other => panic!("expected oldest surviving Batch, got {other:?}"),
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
