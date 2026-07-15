//! Per-feature min/max normalization based on the generated dataset.

use std::marker::PhantomData;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::data::sample::TASK_FEATURE_DIM;

const EPSILON: f64 = 1.0e-8;

/// Marker type for a [`NormalizationParams`] that is still accumulating min/max
/// statistics from observed samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collecting;

/// Marker type for a [`NormalizationParams`] that has been finalized and can
/// normalize or save data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ready;

/// Running min/max statistics for all input features.
///
/// The `State` type parameter enforces the lifecycle:
/// [`Collecting`] → [`finalize`] → [`Ready`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct NormalizationParams<State = Ready> {
    pub mins: Vec<f64>,
    pub maxs: Vec<f64>,
    #[serde(skip)]
    _state: PhantomData<State>,
}

impl NormalizationParams<Collecting> {
    /// Create empty statistics ready to observe samples.
    pub fn new() -> Self {
        Self {
            mins: vec![f64::INFINITY; TASK_FEATURE_DIM],
            maxs: vec![f64::NEG_INFINITY; TASK_FEATURE_DIM],
            _state: PhantomData,
        }
    }

    /// Update running min/max with a single feature vector.
    pub fn update(&mut self, features: &[f64]) {
        for (i, &v) in features.iter().enumerate() {
            self.mins[i] = self.mins[i].min(v);
            self.maxs[i] = self.maxs[i].max(v);
        }
    }

    /// Finalize the observed statistics into a ready-to-use normalization table.
    pub fn finalize(self) -> NormalizationParams<Ready> {
        NormalizationParams {
            mins: self.mins,
            maxs: self.maxs,
            _state: PhantomData,
        }
    }
}

impl NormalizationParams<Ready> {
    /// Normalize raw features to the [0, 1] range.
    pub fn normalize(&self, features: &[f64]) -> Vec<f32> {
        features
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let range = self.maxs[i] - self.mins[i];
                let scaled = if range > EPSILON {
                    (v - self.mins[i]) / range
                } else {
                    0.0
                };
                scaled as f32
            })
            .collect()
    }

    /// Persist the normalization parameters to JSON.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json =
            serde_json::to_string(self).context("Failed to serialize normalization params")?;
        std::fs::write(path, json).with_context(|| {
            format!("Failed to write normalization params to {}", path.display())
        })?;
        Ok(())
    }

    /// Load previously saved normalization parameters from JSON.
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read normalization params from {}",
                path.display()
            )
        })?;
        serde_json::from_str(&json).context("Failed to parse normalization params")
    }
}

impl Default for NormalizationParams<Collecting> {
    fn default() -> Self {
        Self::new()
    }
}
