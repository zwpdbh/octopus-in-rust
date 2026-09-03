//! Loader for `faf-datagen` output directories:
//!   images/000000.png, labels/000000.txt (YOLO: `<class_id> <cx> <cy> <w> <h>`
//!   normalized), classes.txt (line number = class id).
//!
//! Images are decoded lazily per batch from the host (thousands of 640×640
//! frames don't fit one GPU buffer — the Fashion-MNIST lesson); anchor
//! targets are computed on the host in plain Rust and uploaded as tensors.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use burn::tensor::backend::Backend;
use burn::tensor::{Device, Int, Tensor, TensorData};

use crate::anchors::CenterBox;
use crate::matching::AnchorTargets;

/// Positive-match IoU threshold (plan: pos if IoU ≥ 0.5; every GT also
/// force-matched to its best anchor — see `matching::match_anchors`).
pub const POS_IOU_THRESHOLD: f32 = 0.5;

/// One ground-truth box of one image: 0-based class id + normalized box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GtBox {
    pub class_id: usize,
    pub bbox: CenterBox,
}

/// One dataset sample: image path + its parsed labels.
#[derive(Debug, Clone)]
pub struct Sample {
    pub image_path: PathBuf,
    pub gt: Vec<GtBox>,
}

/// A faf-datagen dataset directory, loaded (labels eagerly, images lazily).
#[derive(Debug, Clone)]
pub struct DetectDataset {
    pub classes: Vec<String>,
    pub samples: Vec<Sample>,
    pub input_size: u32,
}

/// One assembled training batch on the target device.
pub struct TrainBatch<B: Backend> {
    pub images: Tensor<B, 4>,
    pub cls_targets: Tensor<B, 2, Int>,
    pub box_targets: Tensor<B, 3>,
    pub pos_mask: Tensor<B, 2>,
}

impl DetectDataset {
    /// Load `classes.txt` + `labels/*.txt`, pairing each label file with its
    /// `images/<stem>.png`. Label files sort lexicographically (zero-padded
    /// stems keep this numeric).
    pub fn load(dir: &Path, input_size: u32) -> Result<Self> {
        let classes_text = fs::read_to_string(dir.join("classes.txt"))
            .with_context(|| format!("reading {}", dir.join("classes.txt").display()))?;
        let classes: Vec<String> = classes_text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        anyhow::ensure!(
            !classes.is_empty(),
            "classes.txt is empty in {}",
            dir.display()
        );

        let labels_dir = dir.join("labels");
        let mut label_files: Vec<PathBuf> = fs::read_dir(&labels_dir)
            .with_context(|| format!("reading {}", labels_dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
            .collect();
        label_files.sort();

        let mut samples = Vec::with_capacity(label_files.len());
        for label_path in label_files {
            let stem = label_path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("label file without a valid stem")?
                .to_string();
            let image_path = dir.join("images").join(format!("{stem}.png"));
            anyhow::ensure!(
                image_path.exists(),
                "label {} has no image {}",
                label_path.display(),
                image_path.display()
            );
            let text = fs::read_to_string(&label_path)
                .with_context(|| format!("reading {}", label_path.display()))?;
            let gt = parse_yolo_labels(&text, classes.len())
                .with_context(|| format!("parsing {}", label_path.display()))?;
            samples.push(Sample { image_path, gt });
        }
        anyhow::ensure!(
            !samples.is_empty(),
            "no labels found in {}",
            labels_dir.display()
        );

        Ok(Self {
            classes,
            samples,
            input_size,
        })
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Assemble one training batch: decode the images, compute anchor targets
    /// on the host, upload everything to `device`.
    pub fn load_batch<B: Backend>(
        &self,
        indices: &[usize],
        anchors: &[CenterBox],
        device: &Device<B>,
    ) -> Result<TrainBatch<B>> {
        let batch = indices.len();
        let n_anchors = anchors.len();
        let side = self.input_size as usize;

        let mut pixels = Vec::with_capacity(batch * 3 * side * side);
        let mut cls = Vec::with_capacity(batch * n_anchors);
        let mut offsets = Vec::with_capacity(batch * n_anchors * 4);
        let mut mask = Vec::with_capacity(batch * n_anchors);

        for &i in indices {
            let sample = &self.samples[i];
            pixels.extend(decode_image(&sample.image_path, self.input_size)?);
            let gt: Vec<(usize, CenterBox)> =
                sample.gt.iter().map(|g| (g.class_id, g.bbox)).collect();
            let targets = AnchorTargets::build(anchors, &gt, POS_IOU_THRESHOLD);
            cls.extend(targets.cls);
            offsets.extend(targets.offsets.into_iter().flatten());
            mask.extend(targets.pos_mask);
        }

        Ok(TrainBatch {
            images: Tensor::from_data(TensorData::new(pixels, [batch, 3, side, side]), device),
            cls_targets: Tensor::from_data(TensorData::new(cls, [batch, n_anchors]), device),
            box_targets: Tensor::from_data(TensorData::new(offsets, [batch, n_anchors, 4]), device),
            pos_mask: Tensor::from_data(TensorData::new(mask, [batch, n_anchors]), device),
        })
    }
}

/// Parse YOLO label text into normalized center-form GT boxes.
fn parse_yolo_labels(text: &str, num_classes: usize) -> Result<Vec<GtBox>> {
    let mut out = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        anyhow::ensure!(
            fields.len() == 5,
            "line {}: expected 5 fields <class_id> <cx> <cy> <w> <h>",
            lineno + 1
        );
        let class_id: usize = fields[0]
            .parse()
            .with_context(|| format!("line {}: bad class id", lineno + 1))?;
        anyhow::ensure!(
            class_id < num_classes,
            "line {}: class id {class_id} out of range ({num_classes} classes)",
            lineno + 1
        );
        let f = |i: usize| -> Result<f32> {
            fields[i]
                .parse()
                .with_context(|| format!("line {}: bad coordinate", lineno + 1))
        };
        out.push(GtBox {
            class_id,
            bbox: CenterBox {
                cx: f(1)?,
                cy: f(2)?,
                w: f(3)?,
                h: f(4)?,
            },
        });
    }
    Ok(out)
}

/// Decode one PNG into CHW f32 pixels, normalized to 0..=1 (datagen `/255`).
/// Fails loudly on size mismatch — the model is trained at a fixed input size.
fn decode_image(path: &Path, input_size: u32) -> Result<Vec<f32>> {
    let img = image::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .to_rgb8();
    anyhow::ensure!(
        img.width() == input_size && img.height() == input_size,
        "{}: expected {}×{}, got {}×{}",
        path.display(),
        input_size,
        input_size,
        img.width(),
        img.height()
    );
    let raw = img.as_raw();
    let side = input_size as usize;
    let mut out = vec![0.0f32; 3 * side * side];
    for y in 0..side {
        for x in 0..side {
            let p = (y * side + x) * 3;
            for c in 0..3 {
                out[c * side * side + y * side + x] = raw[p + c] as f32 / 255.0;
            }
        }
    }
    Ok(out)
}

/// One image as a (1, 3, H, W) tensor on `device` (predict path).
pub fn image_tensor<B: Backend>(
    path: &Path,
    input_size: u32,
    device: &Device<B>,
) -> Result<Tensor<B, 4>> {
    let side = input_size as usize;
    let pixels = decode_image(path, input_size)?;
    Ok(Tensor::from_data(
        TensorData::new(pixels, [1, 3, side, side]),
        device,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yolo_parse_ok() {
        let gt = parse_yolo_labels("7 0.5 0.25 0.1 0.2\n12 0.0 1.0 0.05 0.05\n", 20).unwrap();
        assert_eq!(gt.len(), 2);
        assert_eq!(gt[0].class_id, 7);
        assert!((gt[0].bbox.cy - 0.25).abs() < 1e-6);
        assert_eq!(gt[1].class_id, 12);
    }

    #[test]
    fn yolo_parse_rejects_bad_input() {
        assert!(parse_yolo_labels("1 0.5 0.5 0.1", 20).is_err());
        assert!(parse_yolo_labels("99 0.5 0.5 0.1 0.1", 20).is_err());
        assert!(parse_yolo_labels("x 0.5 0.5 0.1 0.1", 20).is_err());
        assert!(parse_yolo_labels("1 0.5 0.5 0.1 z", 20).is_err());
    }
}
