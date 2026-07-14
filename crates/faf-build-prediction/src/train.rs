//! Training pipeline for the build-time predictor.

use std::path::Path;

use burn::data::dataloader::DataLoaderBuilder;
use burn::optim::AdamConfig;
use burn::prelude::*;
use burn::record::CompactRecorder;
use burn::tensor::backend::AutodiffBackend;
use burn::train::metric::LossMetric;
use burn::train::{Learner, SupervisedTraining};

use crate::data::dataset::{EcoPlanBatcher, SqliteDataset};
use crate::model::predictor::EcoPredictorConfig;

#[derive(Config, Debug)]
pub struct TrainingConfig {
    pub model: EcoPredictorConfig,
    pub optimizer: AdamConfig,
    #[config(default = 10)]
    pub num_epochs: usize,
    #[config(default = 64)]
    pub batch_size: usize,
    #[config(default = 4)]
    pub num_workers: usize,
    #[config(default = 42)]
    pub seed: u64,
    #[config(default = 1.0e-3)]
    pub learning_rate: f64,
}

/// Train a model and save artifacts to `artifact_dir`.
pub fn train<B: AutodiffBackend>(
    artifact_dir: &str,
    dataset_path: &Path,
    config: TrainingConfig,
    device: B::Device,
) {
    create_artifact_dir(artifact_dir);
    copy_normalization_artifact(dataset_path, artifact_dir);

    config
        .save(format!("{artifact_dir}/config.json"))
        .expect("Config should be saved successfully");

    B::seed(&device, config.seed);

    let batcher = EcoPlanBatcher;

    let dataloader_train = DataLoaderBuilder::new(batcher.clone())
        .batch_size(config.batch_size)
        .shuffle(config.seed)
        .num_workers(config.num_workers)
        .build(SqliteDataset::from_path(dataset_path, 0, 10));

    let dataloader_valid = DataLoaderBuilder::new(batcher)
        .batch_size(config.batch_size)
        .shuffle(config.seed)
        .num_workers(config.num_workers)
        .build(SqliteDataset::from_path(dataset_path, 1, 10));

    let training = SupervisedTraining::new(artifact_dir, dataloader_train, dataloader_valid)
        .metrics((LossMetric::new(),))
        .with_file_checkpointer(CompactRecorder::new())
        .num_epochs(config.num_epochs)
        .summary();

    let model = config.model.init::<B>(&device);
    let result = training.launch(Learner::new(
        model,
        config.optimizer.init(),
        config.learning_rate,
    ));

    result
        .model
        .save_file(format!("{artifact_dir}/model"), &CompactRecorder::new())
        .expect("Trained model should be saved successfully");
}

/// Convenience wrapper that trains on the CPU NdArray backend.
pub fn train_with_ndarray(artifact_dir: &str, dataset_path: &Path, config: TrainingConfig) {
    use burn::backend::{Autodiff, NdArray};
    train::<Autodiff<NdArray>>(artifact_dir, dataset_path, config, Default::default());
}

fn create_artifact_dir(artifact_dir: &str) {
    std::fs::remove_dir_all(artifact_dir).ok();
    std::fs::create_dir_all(artifact_dir).ok();
}

fn copy_normalization_artifact(dataset_path: &Path, artifact_dir: &str) {
    let source = dataset_path.with_extension("norm.json");
    let dest = Path::new(artifact_dir).join("norm.json");
    if source.exists() {
        let _ = std::fs::copy(&source, dest);
    }
}
