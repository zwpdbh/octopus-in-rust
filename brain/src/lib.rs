pub mod core;
pub mod hooks;
pub mod session;
pub mod tools;

// Preserve the existing public API by re-exporting the core types.
pub use core::registry::ToolSource;
pub use core::{Brain, BrainConfig, BrainError, BrainEvent, ToolRegistry, TurnInput, TurnResult};
pub use tools::{ExtismPluginSource, PluginManifest, WasmPluginTool, discover_plugins};
