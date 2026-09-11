//! Event-driven training loop for the SSD detector (moved out of the
//! deleted `faf-ml-train` CLI).
//!
//! **One event bus**: all training observability flows through [`TrainEvent`];
//! transports (the server's `/ws/training` stream, future loggers) subscribe
//! downstream. Extending metrics later (real mAP, grad norm, per-scale
//! losses) = add a variant here plus one translation in the consumer — this
//! loop never changes.

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

use crate::anchors::{default_anchor_spec, generate_anchors};
use crate::data::DetectDataset;
use crate::loss::ssd_loss;
use crate::model::{DetectorConfig, SsdModel};

/// Model input side length (datagen `size` must match).
pub const INPUT_SIZE: u32 = 640;

/// Training-run parameters.
#[derive(Debug, Clone)]
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
    /// Loop exited (naturally or aborted); the checkpoint is saved.
    Done {
        run_dir: PathBuf,
        duration_secs: u64,
    },
}

/// Command polled by the loop between batches (mirrors the server's
/// pause/resume/stop semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainControl {
    Continue,
    /// Hold position: no progress, no events, keep polling.
    Pause,
    /// Stop after the current batch; still checkpoint.
    Abort,
}

/// Run a full training run, emitting [`TrainEvent`]s and honoring
/// `control()` between batches. Returns the checkpoint run directory.
pub fn train<AB: AutodiffBackend>(
    params: &TrainParams,
    on_event: &mut dyn FnMut(TrainEvent),
    control: &dyn Fn() -> TrainControl,
) -> Result<PathBuf> {
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
            loop {
                match control() {
                    TrainControl::Continue => break,
                    TrainControl::Pause => std::thread::sleep(Duration::from_millis(50)),
                    TrainControl::Abort => {
                        return finish(model, &config, params, started, on_event)
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
            on_event(TrainEvent::Batch {
                epoch: epoch + 1,
                batch: batches,
                total_batches,
                cls_loss: cls_l,
                bbox_loss: box_l,
                total_loss: total,
            });

            let grads = GradientsParams::from_grads(loss.total.backward(), &model);
            model = optim.step(params.lr, model, grads);

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
        on_event(TrainEvent::EpochEnd {
            epoch: epoch + 1,
            total_epochs: params.epochs,
            train_cls: cls_sum / batches as f32,
            train_bbox: box_sum / batches as f32,
            valid_cls,
            valid_bbox,
        });
    }

    finish(model, &config, params, started, on_event)
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

/// Save the checkpoint and emit `TrainEvent::Done`.
fn finish<AB: AutodiffBackend>(
    model: SsdModel<AB>,
    config: &DetectorConfig,
    params: &TrainParams,
    started: Instant,
    on_event: &mut dyn FnMut(TrainEvent),
) -> Result<PathBuf> {
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
    on_event(TrainEvent::Done {
        run_dir: run_dir.clone(),
        duration_secs: started.elapsed().as_secs(),
    });
    Ok(run_dir)
}
