//! Custom validation metrics for the build-time predictor.
//!
//! All metrics operate on raw completion times (seconds), not `log(time)`.

use std::marker::PhantomData;
use std::sync::Arc;

use burn::prelude::*;
use burn::train::metric::state::{FormatOptions, NumericMetricState};
use burn::train::metric::{
    Adaptor, Metric, MetricAttributes, MetricMetadata, MetricName, Numeric, NumericAttributes,
    NumericEntry, SerializedEntry,
};
use burn::train::RegressionOutput;

/// Input type shared by the time-error metrics.
///
/// Predictions and targets are raw completion times in seconds.
pub struct TimeErrorInput<B: Backend> {
    pub predictions: Tensor<B, 2>,
    pub targets: Tensor<B, 2>,
}

impl<B: Backend> TimeErrorInput<B> {
    fn new(predictions: Tensor<B, 2>, targets: Tensor<B, 2>) -> Self {
        Self {
            predictions,
            targets,
        }
    }
}

impl<B: Backend> Adaptor<TimeErrorInput<B>> for RegressionOutput<B> {
    fn adapt(&self) -> TimeErrorInput<B> {
        // The model predicts log(time); convert back to raw seconds for metrics.
        TimeErrorInput::new(self.output.clone().exp(), self.targets.clone().exp())
    }
}

/// Mean absolute error on raw completion time, in seconds.
#[derive(Clone)]
pub struct MeanAbsoluteErrorMetric<B: Backend> {
    name: Arc<String>,
    state: NumericMetricState,
    _b: PhantomData<B>,
}

impl<B: Backend> Default for MeanAbsoluteErrorMetric<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> MeanAbsoluteErrorMetric<B> {
    /// Create the metric.
    pub fn new() -> Self {
        Self {
            name: Arc::new("MAE".to_string()),
            state: NumericMetricState::default(),
            _b: PhantomData,
        }
    }
}

impl<B: Backend> Metric for MeanAbsoluteErrorMetric<B> {
    type Input = TimeErrorInput<B>;

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let [batch_size, _] = item.predictions.dims();
        let abs_error = (item.predictions.clone() - item.targets.clone()).abs();
        let mae = abs_error
            .mean()
            .into_data()
            .iter::<f64>()
            .next()
            .unwrap_or(f64::NAN);

        self.state.update(
            mae,
            batch_size,
            FormatOptions::new(self.name()).precision(2).unit("s"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("s".to_string()),
            higher_is_better: false,
        }
        .into()
    }
}

impl<B: Backend> Numeric for MeanAbsoluteErrorMetric<B> {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Median absolute error on raw completion time, in seconds.
#[derive(Clone, Default)]
pub struct MedianAbsoluteErrorMetric<B: Backend> {
    name: Arc<String>,
    values: Vec<f64>,
    current: f64,
    current_count: usize,
    _b: PhantomData<B>,
}

impl<B: Backend> MedianAbsoluteErrorMetric<B> {
    /// Create the metric.
    pub fn new() -> Self {
        Self {
            name: Arc::new("MedianAE".to_string()),
            values: Vec::new(),
            current: f64::NAN,
            current_count: 0,
            _b: PhantomData,
        }
    }
}

impl<B: Backend> Metric for MedianAbsoluteErrorMetric<B> {
    type Input = TimeErrorInput<B>;

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let abs_error = (item.predictions.clone() - item.targets.clone()).abs();
        let batch_values: Vec<f64> = abs_error.into_data().iter::<f64>().collect();
        self.current = median(&batch_values);
        self.current_count = batch_values.len();
        self.values.extend(batch_values);

        let running = median(&self.values);

        let formatted = format!("epoch {running:.2} s - batch {:.2} s", self.current);
        let serialized = NumericEntry::Aggregated {
            aggregated_value: self.current,
            count: self.current_count,
        }
        .serialize();

        SerializedEntry::new(formatted, serialized)
    }

    fn clear(&mut self) {
        self.values.clear();
        self.current = f64::NAN;
        self.current_count = 0;
    }

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("s".to_string()),
            higher_is_better: false,
        }
        .into()
    }
}

impl<B: Backend> Numeric for MedianAbsoluteErrorMetric<B> {
    fn value(&self) -> NumericEntry {
        NumericEntry::Aggregated {
            aggregated_value: self.current,
            count: self.current_count,
        }
    }

    fn running_value(&self) -> NumericEntry {
        NumericEntry::Aggregated {
            aggregated_value: median(&self.values),
            count: self.values.len(),
        }
    }
}

/// Mean relative error `|pred - true| / true`.
#[derive(Clone)]
pub struct MeanRelativeErrorMetric<B: Backend> {
    name: Arc<String>,
    state: NumericMetricState,
    _b: PhantomData<B>,
}

impl<B: Backend> Default for MeanRelativeErrorMetric<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> MeanRelativeErrorMetric<B> {
    /// Create the metric.
    pub fn new() -> Self {
        Self {
            name: Arc::new("RelativeError".to_string()),
            state: NumericMetricState::default(),
            _b: PhantomData,
        }
    }
}

impl<B: Backend> Metric for MeanRelativeErrorMetric<B> {
    type Input = TimeErrorInput<B>;

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let [batch_size, _] = item.predictions.dims();
        let rel_error = (item.predictions.clone() - item.targets.clone()).abs()
            / item.targets.clone().clamp_min(1e-6);
        let mean_rel_error = rel_error
            .mean()
            .into_data()
            .iter::<f64>()
            .next()
            .unwrap_or(f64::NAN);

        self.state.update(
            mean_rel_error,
            batch_size,
            FormatOptions::new(self.name()).precision(4),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: None,
            higher_is_better: false,
        }
        .into()
    }
}

impl<B: Backend> Numeric for MeanRelativeErrorMetric<B> {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Fraction of predictions within a relative error threshold.
#[derive(Clone)]
pub struct WithinThresholdMetric<B: Backend> {
    name: Arc<String>,
    threshold: f64,
    state: NumericMetricState,
    _b: PhantomData<B>,
}

impl<B: Backend> WithinThresholdMetric<B> {
    /// Create the metric for the given relative-error threshold.
    pub fn new(threshold: f64) -> Self {
        let name = Arc::new(format!("Within{:.0}%", threshold * 100.0));
        Self {
            name,
            threshold,
            state: NumericMetricState::default(),
            _b: PhantomData,
        }
    }
}

impl<B: Backend> Metric for WithinThresholdMetric<B> {
    type Input = TimeErrorInput<B>;

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let [batch_size, _] = item.predictions.dims();
        let rel_error = (item.predictions.clone() - item.targets.clone()).abs()
            / item.targets.clone().clamp_min(1e-6);
        let within = rel_error
            .lower_elem(self.threshold)
            .float()
            .mean()
            .into_data()
            .iter::<f64>()
            .next()
            .unwrap_or(f64::NAN);

        self.state.update(
            within,
            batch_size,
            FormatOptions::new(self.name()).precision(4),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: None,
            higher_is_better: true,
        }
        .into()
    }
}

impl<B: Backend> Numeric for WithinThresholdMetric<B> {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::NdArray;
    use burn::data::dataloader::Progress;
    use burn::train::metric::Metric;

    fn fake_metadata() -> MetricMetadata {
        MetricMetadata {
            progress: Progress {
                items_processed: 1,
                items_total: 1,
            },
            global_progress: Progress {
                items_processed: 0,
                items_total: 1,
            },
            iteration: Some(0),
            lr: None,
        }
    }

    #[test]
    fn median_computes_correctly() {
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-9);
        assert!((median(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn mean_absolute_error_matches_expected() {
        let device = Default::default();
        let predictions = Tensor::<NdArray, 2>::from_data([[10.0], [20.0], [30.0]], &device);
        let targets = Tensor::<NdArray, 2>::from_data([[12.0], [18.0], [35.0]], &device);
        let input = TimeErrorInput::new(predictions, targets);

        let mut metric = MeanAbsoluteErrorMetric::<NdArray>::new();
        metric.update(&input, &fake_metadata());

        let value = metric.value().current();
        assert!((value - 3.0).abs() < 1e-6);
    }

    #[test]
    fn within_threshold_counts_correctly() {
        let device = Default::default();
        // rel errors: 0.0, 0.2, 0.5, 1.0
        let predictions = Tensor::<NdArray, 2>::from_data([[10.0], [8.0], [5.0], [0.0]], &device);
        let targets = Tensor::<NdArray, 2>::from_data([[10.0], [10.0], [10.0], [10.0]], &device);
        let input = TimeErrorInput::new(predictions, targets);

        let mut metric = WithinThresholdMetric::<NdArray>::new(0.25);
        metric.update(&input, &fake_metadata());

        let value = metric.value().current();
        assert!((value - 0.5).abs() < 1e-6);
    }
}
