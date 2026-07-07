//! Tensor helpers and probability utilities for training.

use burn::tensor::{Tensor, TensorData};

use super::{TrainBackend, TrainDevice};

/// Build a 2-D tensor of shape `[1, features.len()]` from a feature vector.
pub(crate) fn tensor1d_from_vec(features: &[f32]) -> Tensor<TrainBackend, 2> {
    let device: TrainDevice = Default::default();
    let data = TensorData::new(features.to_vec(), [1, features.len()]);
    Tensor::<TrainBackend, 2>::from_data(data, &device)
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
