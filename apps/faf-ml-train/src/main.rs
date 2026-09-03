// The Wgpu backend's types are enormous (wgpu-core validation structs), and
// `optim.step` over a Module hits rustc's default trait-solver recursion limit.
#![recursion_limit = "256"]

//! faf-ml-train — train / predict CLI for the SSD-style icon detector
//! (`faf-ml-model`). The user runs real training manually; `--max-batches`
//! exists for time-boxed smoke runs.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use burn::module::{AutodiffModule, Module};
use burn::optim::grad_clipping::GradientClippingConfig;
use burn::optim::{AdamConfig, GradientsParams, Optimizer};
use burn::record::CompactRecorder;
use burn::tensor::backend::AutodiffBackend;
use burn::tensor::{Device, ElementConversion};
use clap::{Args, Parser, Subcommand};
use rand::seq::SliceRandom;

use faf_ml_model::anchors::{default_anchor_spec, generate_anchors};
use faf_ml_model::data::{image_tensor, DetectDataset};
use faf_ml_model::loss::ssd_loss;
use faf_ml_model::model::{DetectorConfig, SsdModel};
use faf_ml_model::predict::{draw_detections, load_rgb, predict};

const INPUT_SIZE: u32 = 640;
/// d2l §14.7-style: per-class NMS at inference.
const NMS_IOU: f32 = 0.45;

#[derive(Parser)]
#[command(name = "faf-ml-train", about = "Train / run the FAF icon detector")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Train on a faf-datagen directory; checkpoints to <out>/<timestamp>/
    Train(TrainArgs),
    /// Run a checkpoint on one image; prints detections, draws a preview
    Predict(PredictArgs),
}

#[derive(Args)]
struct TrainArgs {
    /// faf-datagen output directory (images/ + labels/ + classes.txt)
    #[arg(long, default_value = "data/faf-detect")]
    data: PathBuf,
    #[arg(long, default_value_t = 50)]
    epochs: usize,
    /// Batch size. Default 4, not the classic 16: with 31900 anchors × 194
    /// classes, the per-scale head outputs and CE logits for one batch must
    /// each stay under cubecl-wgpu's ~128 MB per-buffer page cap (batch 4
    /// peaks at ~100 MB; batch 6+ overflows). Raise carefully on other GPUs.
    #[arg(long, default_value_t = 4)]
    batch: usize,
    #[arg(long, default_value_t = 1e-3)]
    lr: f64,
    /// Cap optimizer steps per epoch (smoke runs; omit for real training)
    #[arg(long)]
    max_batches: Option<usize>,
    /// Run-directory root; each run checkpoints into <out>/<timestamp>/
    #[arg(long, default_value = "data/faf-ml/runs")]
    out: PathBuf,
    /// Use the portable CPU (NdArray) backend instead of Wgpu/Vulkan
    #[arg(long)]
    cpu: bool,
}

#[derive(Args)]
struct PredictArgs {
    /// Run directory containing config.json + model.mpk
    #[arg(long)]
    model: PathBuf,
    /// Input image (must match the checkpoint's input size)
    #[arg(long)]
    image: PathBuf,
    /// Optional path for an annotated preview PNG
    #[arg(long)]
    out: Option<PathBuf>,
    /// Minimum class score to keep a detection (pre-NMS)
    #[arg(long, default_value_t = 0.3)]
    score_threshold: f32,
    /// Use the portable CPU (NdArray) backend instead of Wgpu/Vulkan
    #[arg(long)]
    cpu: bool,
}

/// Compute-backend selection (octopus style: enum, not a stringly flag).
enum ComputeBackend {
    Wgpu,
    NdArrayCpu,
}

impl ComputeBackend {
    fn from_flag(cpu: bool) -> Self {
        if cpu {
            Self::NdArrayCpu
        } else {
            Self::Wgpu
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Train(args) => match ComputeBackend::from_flag(args.cpu) {
            ComputeBackend::Wgpu => train::<faf_ml_model::AdB>(&args),
            ComputeBackend::NdArrayCpu => train::<faf_ml_model::CpuAdB>(&args),
        },
        Command::Predict(args) => match ComputeBackend::from_flag(args.cpu) {
            ComputeBackend::Wgpu => predict_impl::<faf_ml_model::B>(&args),
            ComputeBackend::NdArrayCpu => predict_impl::<faf_ml_model::CpuB>(&args),
        },
    }
}

fn train<AB: AutodiffBackend>(args: &TrainArgs) -> Result<()> {
    let device: Device<AB> = Default::default();
    let dataset = DetectDataset::load(&args.data, INPUT_SIZE)?;
    let anchor_spec = default_anchor_spec();
    let anchors = generate_anchors(&anchor_spec, INPUT_SIZE);
    println!(
        "dataset: {} images, {} classes; {} anchors over {} scales",
        dataset.len(),
        dataset.classes.len(),
        anchors.len(),
        anchor_spec.len()
    );

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

    let smoke = args.max_batches.is_some();
    for epoch in 0..args.epochs {
        let mut order: Vec<usize> = (0..dataset.len()).collect();
        order.shuffle(&mut rand::rng());

        let mut cls_sum = 0.0f32;
        let mut box_sum = 0.0f32;
        let mut batches = 0usize;
        for chunk in order.chunks(args.batch.max(1)) {
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
            if smoke {
                println!(
                    "  epoch {} batch {}: loss {:.4} (cls {:.4}, bbox {:.4})",
                    epoch + 1,
                    batches,
                    total,
                    cls_l,
                    box_l
                );
            }

            let grads = GradientsParams::from_grads(loss.total.backward(), &model);
            model = optim.step(args.lr, model, grads);

            if args.max_batches.is_some_and(|m| batches >= m) {
                break;
            }
        }
        println!(
            "epoch {}: cls {:.4}, bbox {:.4} ({} batches)",
            epoch + 1,
            cls_sum / batches as f32,
            box_sum / batches as f32,
            batches
        );
    }

    let run_dir = args
        .out
        .join(chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string());
    fs::create_dir_all(&run_dir)?;
    model
        .valid()
        .save_file(run_dir.join("model"), &CompactRecorder::new())
        .map_err(|e| anyhow::anyhow!("saving model: {e}"))?;
    fs::write(
        run_dir.join("config.json"),
        serde_json::to_string_pretty(&config)?,
    )?;
    println!("checkpoint → {}", run_dir.display());
    Ok(())
}

/// Inference-backend predict. Separate from the autodiff train path so the
/// checkpoint loads straight onto the inner (non-ad) backend.
fn predict_impl<B: burn::tensor::backend::Backend>(args: &PredictArgs) -> Result<()> {
    let device: Device<B> = Default::default();
    let config_text = fs::read_to_string(args.model.join("config.json"))
        .with_context(|| format!("reading {}", args.model.join("config.json").display()))?;
    let config: DetectorConfig = serde_json::from_str(&config_text)?;
    let anchors = generate_anchors(&config.anchors, config.input_size);

    let model = SsdModel::<B>::new(&config, &device)
        .load_file(args.model.join("model"), &CompactRecorder::new(), &device)
        .map_err(|e| anyhow::anyhow!("loading model: {e}"))?;

    let image = image_tensor::<B>(&args.image, config.input_size, &device)?;
    let detections = predict(&model, &anchors, image, args.score_threshold, NMS_IOU);
    println!(
        "{} detections (score ≥ {}):",
        detections.len(),
        args.score_threshold
    );
    for det in &detections {
        let w = config.input_size as f32;
        println!(
            "  {:30} score {:.3}  box [{:.0}, {:.0}, {:.0}, {:.0}]",
            config.classes.get(det.class_id).map_or("?", String::as_str),
            det.score,
            det.bbox.x1 * w,
            det.bbox.y1 * w,
            det.bbox.x2 * w,
            det.bbox.y2 * w,
        );
    }

    if let Some(out) = &args.out {
        let annotated = draw_detections(&load_rgb(&args.image)?, &detections);
        annotated.save(out)?;
        println!("preview → {}", out.display());
    }
    Ok(())
}
