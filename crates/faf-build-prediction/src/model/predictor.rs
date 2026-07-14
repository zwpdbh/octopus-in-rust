//! Sequence regression model predicting `log(completion_time)` from a plan.
//!
//! The model processes each `BuildTask` as a step in a sequence using an LSTM.
//! The final hidden state is projected to a single log-time value.

use burn::nn::loss::{MseLoss, Reduction};
use burn::nn::{Linear, LinearConfig, Lstm, LstmConfig};
use burn::prelude::*;
use burn::tensor::backend::AutodiffBackend;
use burn::train::{InferenceStep, RegressionOutput, TrainOutput, TrainStep};

use crate::data::dataset::EcoPlanBatch;
use crate::data::sample::TASK_FEATURE_DIM;

/// Configuration for the predictor network.
#[derive(Config, Debug)]
pub struct EcoPredictorConfig {
    /// Size of the LSTM hidden state.
    #[config(default = 128)]
    pub hidden_size: usize,
}

impl EcoPredictorConfig {
    /// Initialize the model on the given device.
    pub fn init<B: Backend>(&self, device: &B::Device) -> EcoPredictor<B> {
        EcoPredictor {
            lstm: LstmConfig::new(TASK_FEATURE_DIM, self.hidden_size, true)
                .with_batch_first(true)
                .init(device),
            output: LinearConfig::new(self.hidden_size, 1).init(device),
        }
    }
}

#[derive(Module, Debug)]
pub struct EcoPredictor<B: Backend> {
    lstm: Lstm<B>,
    output: Linear<B>,
}

impl<B: Backend> EcoPredictor<B> {
    /// Forward pass returning raw predictions.
    pub fn forward(&self, features: Tensor<B, 3>) -> Tensor<B, 2> {
        // features: [batch, seq, TASK_FEATURE_DIM]
        let (_output, state) = self.lstm.forward(features, None);
        // state.hidden: [batch, hidden_size]
        self.output.forward(state.hidden)
    }

    /// Forward pass packaged as a regression output for Burn training.
    pub fn forward_regression(
        &self,
        features: Tensor<B, 3>,
        targets: Tensor<B, 2>,
    ) -> RegressionOutput<B> {
        let predictions = self.forward(features);
        let loss = MseLoss::new().forward(predictions.clone(), targets.clone(), Reduction::Mean);
        RegressionOutput::new(loss, predictions, targets)
    }
}

impl<B: AutodiffBackend> TrainStep for EcoPredictor<B> {
    type Input = EcoPlanBatch<B>;
    type Output = RegressionOutput<B>;

    fn step(&self, batch: EcoPlanBatch<B>) -> TrainOutput<RegressionOutput<B>> {
        let item = self.forward_regression(batch.features, batch.targets);
        TrainOutput::new(self, item.loss.backward(), item)
    }
}

impl<B: Backend> InferenceStep for EcoPredictor<B> {
    type Input = EcoPlanBatch<B>;
    type Output = RegressionOutput<B>;

    fn step(&self, batch: EcoPlanBatch<B>) -> RegressionOutput<B> {
        self.forward_regression(batch.features, batch.targets)
    }
}
