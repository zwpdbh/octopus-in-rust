//! Event-driven training loop for the SSD detector.
//!
//! **One event bus**: all training observability flows through [`TrainEvent`];
//! transports (the server's `/ws/training` stream, future loggers) subscribe
//! downstream. Extending metrics later (real mAP, grad norm, per-scale
//! losses) = add a variant here plus one translation in the consumer — this
//! loop never changes.
//!
//! Control flows the other way through a `watch` channel ([`ControlState`]):
//! pause/resume/stop/speed are read at batch boundaries only.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use burn::module::{AutodiffModule, Module};
use burn::optim::grad_clipping::GradientClippingConfig;
use burn::optim::{AdamConfig, GradientsParams, Optimizer};
use burn::record::CompactRecorder;
use burn::tensor::backend::AutodiffBackend;
use burn::tensor::{Device, ElementConversion};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::anchors::{default_anchor_spec, generate_anchors};
use crate::data::DetectDataset;
use crate::loss::ssd_loss;
use crate::model::{DetectorConfig, SsdModel};

/// Model input side length (datagen `size` must match).
pub const INPUT_SIZE: u32 = 640;

/// Training-run parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainParams {
    /// Dataset directory (the platform store root; snapshots live under
    /// `datasets/` inside it).
    pub data: PathBuf,
    /// Dataset snapshot name (`datasets/<name>.json`). REQUIRED — training
    /// consumes an immutable snapshot, never the live store.
    pub dataset: String,
    /// Run-directory root; each run checkpoints into `<out>/<timestamp>/`.
    pub out: PathBuf,
    pub epochs: usize,
    /// Batch size — hard-capped at 4 by the GPU buffer limit (see handover
    /// gotchas); raise only with gradient accumulation.
    pub batch: usize,
    pub lr: f64,
    /// Fraction of samples held out for validation (deterministic split).
    pub valid_fraction: f32,
    /// Cap optimizer steps per epoch (smoke runs; `None` = full epochs).
    pub max_batches: Option<usize>,
    /// Use the portable CPU (NdArray) backend instead of Wgpu/Vulkan.
    pub cpu: bool,
}

impl Default for TrainParams {
    fn default() -> Self {
        Self {
            data: PathBuf::from("data/faf-ml"),
            dataset: String::new(),
            out: PathBuf::from("data/faf-ml/runs"),
            epochs: 50,
            batch: 4,
            lr: 1e-3,
            valid_fraction: 0.1,
            max_batches: None,
            cpu: false,
        }
    }
}

/// One observable training event (the single metrics channel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrainEvent {
    /// One optimizer step's losses.
    Batch {
        epoch: usize,
        batch: usize,
        total_batches: usize,
        cls_loss: f32,
        bbox_loss: f32,
        total_loss: f32,
    },
    /// Epoch finished: train means + no-grad validation means.
    EpochEnd {
        epoch: usize,
        total_epochs: usize,
        train_cls: f32,
        train_bbox: f32,
        valid_cls: f32,
        valid_bbox: f32,
    },
    /// The thread has actually entered the pause hold (first observation of
    /// `TrainAction::Pause` at a batch boundary). Lets the manager tell
    /// "pausing" (command sent) from "paused" (thread is holding).
    Paused,
}

/// Action the loop should take at the next batch boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainAction {
    Continue,
    /// Hold position: no progress, no events, keep polling.
    Pause,
    /// Stop after the current batch; still checkpoint.
    Abort,
}

/// Control channel value read between batches (one watch value for both
/// pause/stop and the speed throttle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlState {
    pub action: TrainAction,
    /// Post-batch throttle in batches/sec (≤ 0 = unlimited).
    pub batches_per_sec: f64,
}

/// How a training run ended (terminal state — the loop emits no `Done`
/// event; the return value IS the terminal signal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrainExit {
    /// All epochs finished.
    Completed {
        run_dir: PathBuf,
        duration_secs: u64,
    },
    /// Aborted via `TrainAction::Abort` (Stop or Reset) — checkpoint saved.
    Aborted {
        run_dir: PathBuf,
        duration_secs: u64,
    },
}

/// Run a full training run, emitting [`TrainEvent`]s over `events` and
/// honoring `control` between batches. Returns how the run ended (the
/// checkpoint is saved in both exit cases).
pub fn train<AB: AutodiffBackend>(
    params: &TrainParams,
    events: mpsc::UnboundedSender<TrainEvent>,
    control: watch::Receiver<ControlState>,
) -> Result<TrainExit> {
    let started = Instant::now();
    let device: Device<AB> = Default::default();
    anyhow::ensure!(
        !params.dataset.trim().is_empty(),
        "no dataset snapshot selected — create one on the Datasets page first"
    );
    let dataset = DetectDataset::load_snapshot(&params.data, &params.dataset, INPUT_SIZE)?;
    anyhow::ensure!(
        dataset.len() >= 2,
        "need at least 2 samples to train (have {})",
        dataset.len()
    );
    let anchor_spec = default_anchor_spec();
    let anchors = generate_anchors(&anchor_spec, INPUT_SIZE);

    // Deterministic train/valid split over sample indices.
    let valid_count =
        ((dataset.len() as f32 * params.valid_fraction).round() as usize).min(dataset.len() - 1);
    let mut indices: Vec<usize> = (0..dataset.len()).collect();
    indices.shuffle(&mut rand::rng());
    let (valid_idx, train_idx) = indices.split_at(valid_count);
    let valid_idx: Vec<usize> = valid_idx.to_vec();
    let train_idx: Vec<usize> = train_idx.to_vec();
    let batches_per_epoch = train_idx.len().div_ceil(params.batch.max(1));
    let total_batches = batches_per_epoch * params.epochs;

    let config = DetectorConfig {
        input_size: INPUT_SIZE,
        classes: dataset.classes.clone(),
        anchors: anchor_spec,
        backbone_channels: vec![64, 128, 256, 256],
    };
    let mut model = SsdModel::<AB>::new(&config, &device);
    let mut optim = AdamConfig::new()
        .with_grad_clipping(Some(GradientClippingConfig::Norm(1.0)))
        .init::<AB, SsdModel<AB>>();

    for epoch in 0..params.epochs {
        let mut order = train_idx.clone();
        order.shuffle(&mut rand::rng());

        let mut cls_sum = 0.0f32;
        let mut box_sum = 0.0f32;
        let mut batches = 0usize;
        for chunk in order.chunks(params.batch.max(1)) {
            // Honor control between batches (pause holds without progress).
            let mut paused = false;
            loop {
                match control.borrow().action {
                    TrainAction::Continue => break,
                    TrainAction::Pause => {
                        if !paused {
                            paused = true;
                            let _ = events.send(TrainEvent::Paused);
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    TrainAction::Abort => {
                        let (run_dir, duration_secs) = finish(model, &config, params, started)?;
                        return Ok(TrainExit::Aborted {
                            run_dir,
                            duration_secs,
                        });
                    }
                }
            }

            let batch = dataset.load_batch::<AB>(chunk, &anchors, &device)?;
            let (cls_logits, box_preds) = model.forward(batch.images);
            let loss = ssd_loss(
                cls_logits,
                box_preds,
                batch.cls_targets,
                batch.box_targets,
                batch.pos_mask,
                &device,
            );
            let total: f32 = loss.total.clone().into_scalar().elem();
            let cls_l: f32 = loss.cls.clone().into_scalar().elem();
            let box_l: f32 = loss.bbox.clone().into_scalar().elem();
            anyhow::ensure!(
                total.is_finite(),
                "non-finite loss at epoch {} batch {} (cls {cls_l}, bbox {box_l})",
                epoch + 1,
                batches + 1
            );
            cls_sum += cls_l;
            box_sum += box_l;
            batches += 1;
            let _ = events.send(TrainEvent::Batch {
                epoch: epoch + 1,
                batch: batches,
                total_batches,
                cls_loss: cls_l,
                bbox_loss: box_l,
                total_loss: total,
            });

            let grads = GradientsParams::from_grads(loss.total.backward(), &model);
            model = optim.step(params.lr, model, grads);

            // Post-batch throttle (0 or negative = unlimited).
            let limit = control.borrow().batches_per_sec;
            if limit > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(1.0 / limit));
            }

            if params.max_batches.is_some_and(|m| batches >= m) {
                break;
            }
        }

        // Epoch end: no-grad validation pass over the held-out split.
        let (valid_cls, valid_bbox) = if valid_idx.is_empty() {
            (f32::NAN, f32::NAN)
        } else {
            eval_valid::<AB>(
                &dataset,
                &valid_idx,
                &anchors,
                &model,
                params.batch,
                &device,
            )?
        };
        let _ = events.send(TrainEvent::EpochEnd {
            epoch: epoch + 1,
            total_epochs: params.epochs,
            train_cls: cls_sum / batches as f32,
            train_bbox: box_sum / batches as f32,
            valid_cls,
            valid_bbox,
        });
    }

    let (run_dir, duration_secs) = finish(model, &config, params, started)?;
    Ok(TrainExit::Completed {
        run_dir,
        duration_secs,
    })
}

/// No-grad forward over the validation split → mean (cls, bbox) losses.
fn eval_valid<AB: AutodiffBackend>(
    dataset: &DetectDataset,
    valid_idx: &[usize],
    anchors: &[crate::anchors::CenterBox],
    model: &SsdModel<AB>,
    batch: usize,
    device: &Device<AB>,
) -> Result<(f32, f32)> {
    let valid_model = model.valid();
    let mut cls_sum = 0.0f32;
    let mut box_sum = 0.0f32;
    let mut batches = 0usize;
    for chunk in valid_idx.chunks(batch.max(1)) {
        // Autodiff shares the inner backend's device type, so the same
        // device works for the inference batch.
        let batch_data = dataset.load_batch::<AB::InnerBackend>(chunk, anchors, device)?;
        let (cls_logits, box_preds) = valid_model.forward(batch_data.images);
        let loss = ssd_loss(
            cls_logits,
            box_preds,
            batch_data.cls_targets,
            batch_data.box_targets,
            batch_data.pos_mask,
            device,
        );
        cls_sum += loss.cls.into_scalar().elem::<f32>();
        box_sum += loss.bbox.into_scalar().elem::<f32>();
        batches += 1;
    }
    Ok((cls_sum / batches as f32, box_sum / batches as f32))
}

/// Save the checkpoint; returns the run dir and elapsed seconds.
fn finish<AB: AutodiffBackend>(
    model: SsdModel<AB>,
    config: &DetectorConfig,
    params: &TrainParams,
    started: Instant,
) -> Result<(PathBuf, u64)> {
    let run_dir = params
        .out
        .join(chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string());
    fs::create_dir_all(&run_dir)?;
    model
        .valid()
        .save_file(run_dir.join("model"), &CompactRecorder::new())
        .map_err(|e| anyhow::anyhow!("saving model: {e}"))?;
    fs::write(
        run_dir.join("config.json"),
        serde_json::to_string_pretty(config)?,
    )
    .with_context(|| format!("writing {}", run_dir.join("config.json").display()))?;
    Ok((run_dir, started.elapsed().as_secs()))
}
