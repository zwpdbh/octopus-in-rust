//! Sequence regression model predicting `log(completion_time)` from a plan.
//!
//! The model processes each `BuildTask` as a step in a sequence using an LSTM.
//! The final hidden state is projected to a single log-time value.

use burn::nn::{Dropout, DropoutConfig, Linear, LinearConfig, Lstm, LstmConfig};
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
    /// Dropout probability applied to the final LSTM hidden state.
    #[config(default = 0.0)]
    pub dropout: f64,
}

impl EcoPredictorConfig {
    /// Initialize the model on the given device.
    pub fn init<B: Backend>(&self, device: &B::Device) -> EcoPredictor<B> {
        self.init_with_weight(device, 0.0)
    }

    /// Initialize the model with a per-sample time-based loss weight.
    ///
    /// `time_weight_power` controls how much faster plans are up-weighted in the
    /// training loss. The per-sample weight is:
    ///
    /// ```text
    /// weight = raw_time^{-time_weight_power}
    ///        = exp(-time_weight_power * log_time)
    /// ```
    ///
    /// Because the dataset is dominated by slow / "not practical" plans, a value
    /// of `0.0` (standard MSE) lets the optimizer mostly ignore the rare fast
    /// plans. A positive value makes fast plans contribute more to the gradient,
    /// which usually improves time estimates for practical plans but can bias
    /// the model toward underpredicting if set too high.
    ///
    /// A value of `0.0` disables weighting.
    pub fn init_with_weight<B: Backend>(
        &self,
        device: &B::Device,
        time_weight_power: f64,
    ) -> EcoPredictor<B> {
        EcoPredictor {
            lstm: LstmConfig::new(TASK_FEATURE_DIM, self.hidden_size, true)
                .with_batch_first(true)
                .init(device),
            dropout: DropoutConfig::new(self.dropout).init(),
            output: LinearConfig::new(self.hidden_size, 1).init(device),
            time_weight_power,
        }
    }
}

#[derive(Module, Debug)]
pub struct EcoPredictor<B: Backend> {
    lstm: Lstm<B>,
    dropout: Dropout,
    output: Linear<B>,
    /// Per-sample loss weighting power. Marked `#[module(skip)]` because it is a
    /// training hyperparameter, not a learned parameter, so it must not be saved
    /// or loaded with the model weights.
    #[module(skip)]
    time_weight_power: f64,
}

impl<B: Backend> EcoPredictor<B> {
    /// Forward pass returning raw predictions.
    pub fn forward(&self, features: Tensor<B, 3>) -> Tensor<B, 2> {
        // features: [batch, seq, TASK_FEATURE_DIM]
        let (_output, state) = self.lstm.forward(features, None);
        // state.hidden: [batch, hidden_size]
        let hidden = self.dropout.forward(state.hidden);
        self.output.forward(hidden)
    }

    /// Forward pass packaged as a regression output for Burn training.
    ///
    /// Computes a time-weighted MSE on `log(completion_time)`:
    ///
    /// ```text
    /// loss = mean(weight * (prediction - target)²)
    /// weight = exp(-time_weight_power * target)
    /// ```
    ///
    /// The `target` tensor already stores `log(raw_time)`, so `exp(target)` is
    /// the raw completion time. A positive `time_weight_power` assigns larger
    /// weights to samples with smaller raw times, which up-weights the rare
    /// fast/practical plans during backpropagation. A power of `0.0` gives the
    /// standard unweighted MSE because every weight becomes `1.0`.
    pub fn forward_regression(
        &self,
        features: Tensor<B, 3>,
        targets: Tensor<B, 2>,
    ) -> RegressionOutput<B> {
        let predictions = self.forward(features);
        let diff = predictions.clone() - targets.clone();
        let squared = diff.powf_scalar(2.0);

        // Clamp log-time to >= 0 (raw time >= 1 s) to avoid extreme weights
        // for very small / negative log-times.
        let clamped = targets.clone().clamp(0.0, f64::INFINITY);
        // raw_time = exp(log_time), so:
        // weight = raw_time^{-power} = exp(-power * log_time)
        let weights = clamped.mul_scalar(-self.time_weight_power).exp();

        let loss = (squared * weights).mean();
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
