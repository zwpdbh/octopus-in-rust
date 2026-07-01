//! Tensor helpers and probability utilities for training.

use burn::tensor::{Tensor, TensorData};

use super::{TrainBackend, TrainDevice};

/// Build a 2-D tensor of shape `[1, features.len()]` from a feature vector.
pub(crate) fn tensor1d_from_vec(features: &[f32]) -> Tensor<TrainBackend, 2> {
    let device: TrainDevice = Default::default();
    let data = TensorData::new(features.to_vec(), [1, features.len()]);
    Tensor::<TrainBackend, 2>::from_data(data, &device)
}

/// Gaussian log-likelihood for a scalar action.
pub(crate) fn gaussian_log_prob_scalar(
    mean: Tensor<TrainBackend, 1>,
    action: f32,
    std: f32,
    device: &TrainDevice,
) -> Tensor<TrainBackend, 1> {
    let action_tensor =
        Tensor::<TrainBackend, 1>::from_data(TensorData::new(vec![action], [1]), device);
    let diff = action_tensor - mean;
    let diff_sq = diff.clone() * diff;
    let variance = std * std;
    diff_sq
        .div_scalar(2.0 * variance)
        .neg()
        .add_scalar(-0.5 * (2.0 * std::f32::consts::PI * variance).ln())
}

/// Sum of independent Gaussian log-likelihoods for a vector action.
pub(crate) fn gaussian_log_prob_vec(
    mean: Tensor<TrainBackend, 1>,
    action: &[f32],
    std: f32,
    device: &TrainDevice,
) -> Tensor<TrainBackend, 1> {
    let action_tensor = Tensor::<TrainBackend, 1>::from_data(
        TensorData::new(action.to_vec(), [action.len()]),
        device,
    );
    let diff = action_tensor - mean;
    let diff_sq = diff.clone() * diff;
    let variance = std * std;
    let log_prob = diff_sq
        .div_scalar(2.0 * variance)
        .neg()
        .add_scalar(-0.5 * (2.0 * std::f32::consts::PI * variance).ln());
    log_prob.sum()
}

/// Format a duration in seconds as "Xm Y.Ys", or "-" if `valid` is false.
#[allow(dead_code)]
pub(crate) fn format_time(seconds: f64, valid: bool) -> String {
    if !valid {
        return "-".to_string();
    }
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
}
