//! Regression model predicting `log(completion_time)` from economy + plan features.

use burn::nn::loss::{MseLoss, Reduction};
use burn::nn::{Linear, LinearConfig, Relu};
use burn::prelude::*;
use burn::tensor::backend::AutodiffBackend;
use burn::train::{InferenceStep, RegressionOutput, TrainOutput, TrainStep};

use crate::data::dataset::EcoPlanBatch;
use crate::data::sample::FEATURE_DIM;

/// Configuration for the predictor network.
#[derive(Config, Debug)]
pub struct EcoPredictorConfig {
    /// Size of the first hidden layer.
    #[config(default = 128)]
    pub hidden_size: usize,
    /// Dropout probability (currently unused, kept for future regularization).
    #[config(default = 0.0)]
    pub dropout: f64,
}

impl EcoPredictorConfig {
    /// Initialize the model on the given device.
    pub fn init<B: Backend>(&self, device: &B::Device) -> EcoPredictor<B> {
        EcoPredictor {
            input: LinearConfig::new(FEATURE_DIM, self.hidden_size).init(device),
            hidden: LinearConfig::new(self.hidden_size, self.hidden_size).init(device),
            output: LinearConfig::new(self.hidden_size, 1).init(device),
            activation: Relu::new(),
        }
    }
}

#[derive(Module, Debug)]
pub struct EcoPredictor<B: Backend> {
    input: Linear<B>,
    hidden: Linear<B>,
    output: Linear<B>,
    activation: Relu,
}

impl<B: Backend> EcoPredictor<B> {
    /// Forward pass returning raw predictions.
    pub fn forward(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
        let x = self.input.forward(features);
        let x = self.activation.forward(x);
        let x = self.hidden.forward(x);
        let x = self.activation.forward(x);
        self.output.forward(x)
    }

    /// Forward pass packaged as a regression output for Burn training.
    pub fn forward_regression(
        &self,
        features: Tensor<B, 2>,
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
