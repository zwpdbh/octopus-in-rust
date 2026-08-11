//! Configuration and session state types for the agent chat feature.

use serde::{Deserialize, Serialize};

use crate::components::chat_primitives::ChatMessageItem;

/// Configuration for [`super::AgentChat`] / [`super::use_agent_chat`].
///
/// Only `stream_url` is required; everything else has a sensible default:
///
/// ```rust,ignore
/// // docref: demo
/// let config = AgentChatConfig::new("http://localhost:3000/api/ask/stream");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AgentChatConfig {
    /// URL of the SSE endpoint accepting `POST {"question": "..."}` and
    /// streaming [`super::AgentStreamEvent`] JSON frames.
    pub stream_url: String,
    /// localStorage key used to persist chat sessions. `None` disables
    /// persistence (sessions live in memory only).
    pub storage_key: Option<String>,
    /// Title shown on the welcome screen and in the header of the active chat.
    pub title: String,
    /// Optional subtitle on the welcome screen.
    pub subtitle: Option<String>,
    /// Placeholder text of the composer input.
    pub placeholder: String,
    /// Suggestion chips shown on the welcome screen.
    pub suggestions: Vec<String>,
}

impl AgentChatConfig {
    /// Create a config pointing at the given SSE endpoint.
    pub fn new(stream_url: impl Into<String>) -> Self {
        Self {
            stream_url: stream_url.into(),
            ..Default::default()
        }
    }
}

impl Default for AgentChatConfig {
    fn default() -> Self {
        Self {
            stream_url: String::new(),
            storage_key: None,
            title: "Assistant".to_string(),
            subtitle: None,
            placeholder: "Ask anything...".to_string(),
            suggestions: Vec::new(),
        }
    }
}

/// A persisted chat session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessageItem>,
}

/// All agent chat sessions plus the active selection.
///
/// Field names are part of the persistence format — do not rename, or existing
/// localStorage data will no longer load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentChatSessions {
    pub sessions: Vec<ChatSession>,
    pub active_id: Option<String>,
}

impl AgentChatSessions {
    /// Clear stale streaming flags left behind when a tab was closed
    /// mid-stream (a restored session can never still be streaming).
    pub fn normalize_after_load(&mut self) {
        for session in &mut self.sessions {
            for message in &mut session.messages {
                if let ChatMessageItem::Assistant { is_streaming, .. } = message {
                    *is_streaming = false;
                }
            }
        }
    }
}
