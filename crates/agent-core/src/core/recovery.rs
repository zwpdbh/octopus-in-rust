use std::time::Duration;

use async_trait::async_trait;

use crate::core::errors::BrainError;

/// Action recommended by a [`RecoveryPolicy`] after retries are exhausted.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Refresh the provider and retry the current step once.
    RefreshProvider,
    /// Ask the frontend to provide a new provider (interactive OAuth/device flow).
    RequestInteractiveProvider { reason: String },
    /// Retry after a fixed delay.
    Retry { wait: Duration },
    /// Give up and surface the error.
    Abort,
}

/// Decides what to do when a step fails after all retry attempts.
#[async_trait]
pub trait RecoveryPolicy: Send + Sync {
    async fn recover(&self, error: &BrainError) -> RecoveryAction;
}

/// Default recovery policy:
/// - 401 auth failures → refresh provider.
/// - Transient failures that were not recovered by retries → retry once more.
/// - Everything else → abort.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultRecoveryPolicy;

#[async_trait]
impl RecoveryPolicy for DefaultRecoveryPolicy {
    async fn recover(&self, error: &BrainError) -> RecoveryAction {
        if error.is_auth_failure() {
            return RecoveryAction::RefreshProvider;
        }

        if error.is_transient() {
            return RecoveryAction::Retry {
                wait: Duration::from_secs(1),
            };
        }

        RecoveryAction::Abort
    }
}

/// A recovery policy that never attempts recovery.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRecovery;

#[async_trait]
impl RecoveryPolicy for NoRecovery {
    async fn recover(&self, _error: &BrainError) -> RecoveryAction {
        RecoveryAction::Abort
    }
}
