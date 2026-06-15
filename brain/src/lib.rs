pub mod core;
pub mod hooks;
pub mod session;
pub mod tools;

// Preserve the existing public API by re-exporting the core types.
pub use core::registry::ToolSource;
pub use core::{
    ApprovalPolicy, ApprovalRequest, ApprovalResponse, ApprovalRuntime, AutoApprove, Brain,
    BrainBuilder, BrainConfig, BrainError, BrainEvent, DefaultProviderFactory,
    DefaultRecoveryPolicy, ExponentialBackoffRetryPolicy, ProviderFactory, RecoveryAction,
    RecoveryPolicy, RetryPolicy, ToolRegistry, TurnInput, TurnResult,
};
pub use tools::{ExtismPluginSource, PluginManifest, WasmPluginTool, discover_plugins};
