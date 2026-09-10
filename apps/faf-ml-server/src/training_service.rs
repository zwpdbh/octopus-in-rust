//! Dummy training service: simulates a burn training run on a dedicated
//! thread, emitting metrics events that the `/ws/training` handler streams
//! to the web UI (the fafcn eco-sim pattern: heavy work off-thread, typed
//! events over a channel, commands back in).
//!
//! **SWAP POINT (Phase 2):** replace `run_loop`'s dummy curve math with a
//! call into the real training loop (currently in `apps/faf-ml-train`,
//! to be moved into a library crate). It must emit the same
//! `TrainingServerMessage` events — the WS handler and web page stay
//! unchanged. The thread-per-run + command-channel structure is already
//! what a GPU training loop needs (non-async, cancellable).

use std::{
    sync::mpsc::{channel, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use faf_ml_core::{
    TrainingCommand, TrainingConfig, TrainingMetricsPoint, TrainingServerMessage, TrainingStatus,
};

/// Batches per epoch used to shape dummy runs (~205 store samples at the
/// detector's batch-4 GPU cap).
const BATCHES_PER_EPOCH: usize = 51;

/// Channels to drive and observe a running training thread (mirrors
/// `SimController` in faf-sim-service).
pub struct TrainingController {
    pub cmd_tx: Sender<TrainingCommand>,
    pub event_rx: Receiver<TrainingServerMessage>,
}

pub struct TrainingService;

impl TrainingService {
    /// Spawn a training thread for `config`, ticking at `speed` batches per
    /// wall-clock second.
    pub fn run(config: TrainingConfig, speed: f64) -> TrainingController {
        let (cmd_tx, cmd_rx) = channel::<TrainingCommand>();
        let (event_tx, event_rx) = channel::<TrainingServerMessage>();
        thread::spawn(move || run_loop(config, speed.max(0.1), cmd_rx, event_tx));
        TrainingController { cmd_tx, event_rx }
    }
}

/// Deterministic pseudo-noise in [-0.5, 0.5) — no rand dependency needed
/// for a demo curve.
fn noise(seq: u64) -> f64 {
    (seq.wrapping_mul(2_654_435_761).wrapping_add(891) % 1000) as f64 / 1000.0 - 0.5
}

fn run_loop(
    config: TrainingConfig,
    mut speed: f64,
    cmd_rx: Receiver<TrainingCommand>,
    event_tx: Sender<TrainingServerMessage>,
) {
    let started = Instant::now();
    let total_batches = BATCHES_PER_EPOCH * config.epochs;
    let mut seq = 0u64;
    let mut paused = false;
    let mut stopped = false;

    let _ = event_tx.send(TrainingServerMessage::Status(TrainingStatus::Running));

    'epochs: for epoch in 1..=config.epochs {
        let mut epoch_total = 0.0;
        let mut batch = 1;
        while batch <= BATCHES_PER_EPOCH {
            // Drain pending commands before each tick.
            loop {
                match cmd_rx.try_recv() {
                    Ok(TrainingCommand::Pause) => {
                        paused = true;
                        let _ =
                            event_tx.send(TrainingServerMessage::Status(TrainingStatus::Paused));
                    }
                    Ok(TrainingCommand::Resume) => {
                        paused = false;
                        let _ =
                            event_tx.send(TrainingServerMessage::Status(TrainingStatus::Running));
                    }
                    Ok(TrainingCommand::Stop) => {
                        stopped = true;
                        break 'epochs;
                    }
                    Ok(TrainingCommand::SetSpeed { batches_per_sec }) => {
                        speed = batches_per_sec.max(0.1);
                    }
                    Err(_) => break,
                }
            }
            if paused {
                // Hold position: a paused run neither emits nor advances.
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            // Dummy loss curve: exponential decay + noise floor.
            seq += 1;
            let p = seq as f64 / total_batches as f64;
            let total = 0.08 + 1.2 * (-3.5 * p).exp() + noise(seq) * 0.02;
            let cls = total * 0.7 + noise(seq + 1) * 0.01;
            let bbox = total * 0.3 + noise(seq + 2) * 0.01;
            epoch_total += total;
            if event_tx
                .send(TrainingServerMessage::Metrics(TrainingMetricsPoint {
                    seq,
                    epoch,
                    batch,
                    total_batches,
                    train_loss: total,
                    cls_loss: cls,
                    bbox_loss: bbox,
                    valid_loss: None,
                    map: None,
                }))
                .is_err()
            {
                return; // client gone
            }
            batch += 1;
            thread::sleep(Duration::from_secs_f64(1.0 / speed));
        }

        // Epoch end: one eval point (epoch-mean train loss + valid + mAP).
        seq += 1;
        let mean = epoch_total / BATCHES_PER_EPOCH as f64;
        let p = epoch as f64 / config.epochs as f64;
        if event_tx
            .send(TrainingServerMessage::Metrics(TrainingMetricsPoint {
                seq,
                epoch,
                batch: BATCHES_PER_EPOCH,
                total_batches,
                train_loss: mean,
                cls_loss: mean * 0.7,
                bbox_loss: mean * 0.3,
                valid_loss: Some(mean * 1.08 + noise(seq) * 0.02),
                map: Some(0.62 * (1.0 - (-4.0 * p).exp()) + noise(seq) * 0.005),
            }))
            .is_err()
        {
            return;
        }
    }

    let duration_secs = started.elapsed().as_secs();
    let _ = event_tx.send(TrainingServerMessage::Status(if stopped {
        TrainingStatus::Failed {
            error: "stopped by user".to_string(),
        }
    } else {
        TrainingStatus::Done { duration_secs }
    }));
    // Dropping event_tx signals the WS handler to send `Finished`.
}
