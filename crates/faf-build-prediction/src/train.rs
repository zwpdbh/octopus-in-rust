//! Training pipeline for the build-time predictor.

use std::path::Path;

use burn::data::dataloader::DataLoaderBuilder;
use burn::data::dataset::{Dataset, InMemDataset};
use burn::optim::AdamConfig;
use burn::prelude::*;
use burn::record::CompactRecorder;
use burn::tensor::backend::AutodiffBackend;
use burn::train::metric::LossMetric;
use burn::train::{Learner, SupervisedTraining};

use crate::data::dataset::{EcoPlanBatcher, EcoPlanItem, SqliteDataset};
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
    /// Power used for time-based loss weighting.
    ///
    /// The training loss is MSE on `log(completion_time)`, but each sample is
    /// multiplied by `raw_time^{-time_weight_power}` before averaging. Because
    /// randomly generated plans are overwhelmingly slow, the default value of
    /// `0.0` (standard unweighted MSE) lets the optimizer mostly ignore the rare
    /// fast plans and overpredict their completion times.
    ///
    /// Positive values make fast plans contribute more to the gradient, which
    /// usually improves estimates for practical plans. Values that are too high
    /// can bias the model toward underpredicting slow plans. A good starting
    /// point for a heavily imbalanced dataset is `0.2`–`0.5`.
    #[config(default = 0.0)]
    pub time_weight_power: f64,
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

    // Load all samples and split 80/20 for train/validation.
    let dataset = SqliteDataset::from_path(dataset_path, 0, 1);
    let items: Vec<EcoPlanItem> = dataset.iter().collect();
    let split = (items.len() * 4) / 5;
    let train_items = items[..split].to_vec();
    let valid_items = items[split..].to_vec();

    let batcher = EcoPlanBatcher;

    let dataloader_train = DataLoaderBuilder::new(batcher.clone())
        .batch_size(config.batch_size)
        .shuffle(config.seed)
        .num_workers(config.num_workers)
        .build(InMemDataset::new(train_items));

    let dataloader_valid = DataLoaderBuilder::new(batcher)
        .batch_size(config.batch_size)
        .shuffle(config.seed)
        .num_workers(config.num_workers)
        .build(InMemDataset::new(valid_items));

    let training = SupervisedTraining::new(artifact_dir, dataloader_train, dataloader_valid)
        .metrics((LossMetric::new(),))
        .with_file_checkpointer(CompactRecorder::new())
        .num_epochs(config.num_epochs)
        .summary();

    let model = config
        .model
        .init_with_weight::<B>(&device, config.time_weight_power);
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
