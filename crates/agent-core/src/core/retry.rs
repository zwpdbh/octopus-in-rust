use std::time::Duration;

use async_trait::async_trait;

use crate::core::errors::BrainError;

/// Decides whether a failed step should be retried and how long to wait.
#[async_trait]
pub trait RetryPolicy: Send + Sync {
    /// Maximum number of retry attempts for a single step before recovery
    /// policies are invoked.
    fn max_attempts(&self) -> usize;

    /// Given an error and the current attempt number (1-indexed), return the
    /// wait duration if the step should be retried, or `None` to stop retrying.
    fn should_retry(&self, error: &BrainError, attempt: usize) -> Option<Duration>;
}

/// Exponential backoff with jitter.
#[derive(Debug, Clone)]
pub struct ExponentialBackoffRetryPolicy {
    max_attempts: usize,
    initial_secs: f64,
    max_secs: f64,
    jitter_secs: f64,
}

impl ExponentialBackoffRetryPolicy {
    pub fn new(max_attempts: usize) -> Self {
        Self {
            max_attempts,
            initial_secs: 0.3,
            max_secs: 5.0,
            jitter_secs: 0.5,
        }
    }

    pub fn with_initial_secs(mut self, secs: f64) -> Self {
        self.initial_secs = secs;
        self
    }

    pub fn with_max_secs(mut self, secs: f64) -> Self {
        self.max_secs = secs;
        self
    }

    pub fn with_jitter_secs(mut self, secs: f64) -> Self {
        self.jitter_secs = secs;
        self
    }
}

#[async_trait]
impl RetryPolicy for ExponentialBackoffRetryPolicy {
    fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    fn should_retry(&self, error: &BrainError, attempt: usize) -> Option<Duration> {
        if attempt > self.max_attempts {
            return None;
        }

        if !error.is_transient() && !error.is_auth_failure() {
            return None;
        }

        let base = self.initial_secs * 2_f64.powi((attempt - 1) as i32);
        let capped = base.min(self.max_secs);
        let jitter = rand::random::<f64>() * self.jitter_secs;
        Some(Duration::from_secs_f64(capped + jitter))
    }
}
