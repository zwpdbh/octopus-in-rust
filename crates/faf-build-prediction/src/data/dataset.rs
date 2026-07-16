//! Burn `Dataset` implementation backed by a SQLite database of samples.

use std::path::{Path, PathBuf};

use burn::data::dataset::Dataset;
use burn::prelude::*;
use rusqlite::{Connection, OpenFlags};

use crate::data::normalize::{NormalizationParams, Ready};
use crate::data::sample::TASK_FEATURE_DIM;

/// A single item loaded from the SQLite dataset.
#[derive(Debug, Clone)]
pub struct EcoPlanItem {
    /// Normalized feature vector for the single task in this sample.
    pub features: Vec<f32>,
    /// Target `log(time)` value.
    pub target: f64,
}

/// Burn dataset that reads samples from a SQLite file.
#[derive(Debug, Clone)]
pub struct SqliteDataset {
    path: PathBuf,
    offset: usize,
    len: usize,
    norm: NormalizationParams<Ready>,
}

impl SqliteDataset {
    /// Open a dataset and split it into train/validation portions.
    ///
    /// `split` selects which portion to return; `split_count` determines how
    /// many portions the dataset is divided into.
    pub fn from_path(path: impl Into<PathBuf>, split: usize, split_count: usize) -> Self {
        let path = path.into();
        let total = count_samples(&path).expect("Failed to count dataset rows");
        let norm = load_normalization(&path).expect("Failed to load normalization params");
        let portion = total / split_count;
        let offset = split * portion;
        let len = if split == split_count - 1 {
            total - offset
        } else {
            portion
        };

        Self {
            path,
            offset,
            len,
            norm,
        }
    }
}

impl Dataset<EcoPlanItem> for SqliteDataset {
    fn get(&self, index: usize) -> Option<EcoPlanItem> {
        if index >= self.len {
            return None;
        }
        load_sample(&self.path, &self.norm, self.offset + index)
    }

    fn len(&self) -> usize {
        self.len
    }
}

fn count_samples(path: &PathBuf) -> anyhow::Result<usize> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM samples", [], |row| row.get(0))?;
    Ok(count as usize)
}

fn load_normalization(path: &Path) -> anyhow::Result<NormalizationParams<Ready>> {
    let norm_path = path.with_extension("norm.json");
    NormalizationParams::load(&norm_path)
}

fn load_sample(
    path: &PathBuf,
    norm: &NormalizationParams<Ready>,
    index: usize,
) -> Option<EcoPlanItem> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = conn
        .prepare("SELECT sequence_features, target_time FROM samples LIMIT 1 OFFSET ?")
        .ok()?;

    let row = stmt
        .query_row([index as i64], |row| {
            let features_json: String = row.get(0)?;
            let target: f64 = row.get(1)?;
            Ok((features_json, target))
        })
        .ok()?;

    let raw_features: Vec<[f64; TASK_FEATURE_DIM]> = serde_json::from_str(&row.0).ok()?;
    Some(EcoPlanItem {
        features: normalize(norm, &raw_features),
        target: row.1.ln(),
    })
}

fn normalize(
    norm: &NormalizationParams<Ready>,
    raw_features: &[[f64; TASK_FEATURE_DIM]],
) -> Vec<f32> {
    // The single-task predictor only uses the first task in the plan.
    raw_features
        .first()
        .map(|task| norm.normalize(task))
        .unwrap_or_else(|| vec![0.0; TASK_FEATURE_DIM])
}

/// A Burn batch containing batched feature and target tensors.
#[derive(Clone, Debug)]
pub struct EcoPlanBatch<B: Backend> {
    pub features: Tensor<B, 2>,
    pub targets: Tensor<B, 2>,
}

/// Converts individual `EcoPlanItem`s into a batched tensor.
#[derive(Clone, Default, Debug)]
pub struct EcoPlanBatcher;

impl<B: Backend> burn::data::dataloader::batcher::Batcher<B, EcoPlanItem, EcoPlanBatch<B>>
    for EcoPlanBatcher
{
    fn batch(&self, items: Vec<EcoPlanItem>, device: &B::Device) -> EcoPlanBatch<B> {
        let batch_size = items.len();
        let features: Vec<f32> = items
            .iter()
            .flat_map(|item| item.features.iter().copied())
            .collect();
        let targets: Vec<f32> = items.iter().map(|item| item.target as f32).collect();

        let features = Tensor::<B, 2>::from_data(
            TensorData::new(features, [batch_size, TASK_FEATURE_DIM]).convert::<B::FloatElem>(),
            device,
        );
        let targets = Tensor::<B, 2>::from_data(
            TensorData::new(targets, [batch_size, 1]).convert::<B::FloatElem>(),
            device,
        );

        EcoPlanBatch { features, targets }
    }
}
