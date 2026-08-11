//! Agent chat feature: a config-driven, streaming chat page.
//!
//! Three levels of API:
//! - [`AgentChat`] — batteries-included page component.
//! - [`use_agent_chat`] / [`AgentChatController`] — state hook for custom layouts.
//! - [`stream_agent_events`] / [`AgentStreamEvent`] — low-level SSE client.

mod controller;
mod events;
mod page;
mod sse;
mod state;

pub use controller::{use_agent_chat, AgentChatController};
pub use events::AgentStreamEvent;
pub use page::AgentChat;
pub use sse::stream_agent_events;
pub use state::{AgentChatConfig, AgentChatSessions, ChatSession};
