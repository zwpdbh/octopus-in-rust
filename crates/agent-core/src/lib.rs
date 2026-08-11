pub mod control;
pub mod core;
pub mod hooks;
pub mod session;
pub mod tools;

// Preserve the existing public API by re-exporting the core types.
pub use core::registry::ToolSource;
pub use core::{
    ApiProtocol, ApprovalPolicy, ApprovalRequest, ApprovalResponse, ApprovalRuntime, AutoApprove,
    Brain, BrainBuilder, BrainConfig, BrainError, BrainErrorCategory, BrainEvent, CheckpointPolicy,
    DefaultProviderFactory, DefaultRecoveryPolicy, DefaultSystemPromptPolicy, EventPolicy,
    ExponentialBackoffRetryPolicy, NoOpCheckpointPolicy, NoOpEventPolicy, NoOpStepPolicy,
    NoOpToolResultTransformer, OAuthConfig, OAuthManager, OAuthToken, ProviderFactory,
    ProviderIdentity, ProviderRefreshSender, ProviderType, RecoveryAction, RecoveryPolicy,
    RetryPolicy, StepContext, StepControl, StepOutcome, StepPolicy, SubscriptionProtocol,
    SystemPromptPolicy, ToolAwareSystemPromptPolicy, ToolRegistry, ToolResultTransformer,
    TurnInput, TurnResult,
};
pub use tools::{ExtismPluginSource, PluginManifest, WasmPluginTool, discover_plugins};
