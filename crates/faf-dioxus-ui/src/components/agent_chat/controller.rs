//! Reactive state controller for the agent chat feature.

use dioxus::prelude::*;
use web_sys::Storage;

use super::events::AgentStreamEvent;
use super::sse::stream_agent_events;
use super::state::{AgentChatConfig, AgentChatSessions, ChatSession};
use crate::components::chat_primitives::{ChatHistoryItem, ChatMessageItem, ToolCall};

/// Reactive controller returned by [`use_agent_chat`].
///
/// Holds the session state, composer input, and derived views. Clone it freely
/// to move copies into event handlers.
#[derive(Clone)]
pub struct AgentChatController {
    /// All chat sessions plus the active selection.
    pub sessions: Signal<AgentChatSessions>,
    /// Whether a streaming turn is in flight.
    pub is_loading: Signal<bool>,
    /// Composer input text.
    pub input: Signal<String>,
    /// Messages of the currently active session.
    pub messages: Memo<Vec<ChatMessageItem>>,
    /// Sidebar history items derived from the sessions.
    pub history: Memo<Vec<ChatHistoryItem>>,
    /// Id of the active session, if any.
    pub active_id: Memo<Option<String>>,
    /// Title of the active session (falls back to the configured title).
    pub active_title: Memo<String>,
    config: AgentChatConfig,
}

/// Create the state controller driving an agent chat UI.
///
/// The `config` is captured on first render; later prop changes are ignored.
/// When `config.storage_key` is `Some`, sessions are loaded from and persisted
/// to localStorage under that key.
pub fn use_agent_chat(config: AgentChatConfig) -> AgentChatController {
    let sessions = use_signal({
        let config = config.clone();
        move || match &config.storage_key {
            Some(key) => load_sessions(key),
            None => AgentChatSessions::default(),
        }
    });
    let is_loading = use_signal(|| false);
    let input = use_signal(String::new);

    let messages = use_memo(move || {
        sessions
            .read()
            .active_id
            .as_ref()
            .and_then(|id| {
                sessions
                    .read()
                    .sessions
                    .iter()
                    .find(|s| s.id == *id)
                    .map(|s| s.messages.clone())
            })
            .unwrap_or_default()
    });

    let history = use_memo(move || {
        sessions
            .read()
            .sessions
            .iter()
            .map(|s| ChatHistoryItem {
                id: s.id.clone(),
                title: s.title.clone(),
            })
            .collect::<Vec<_>>()
    });

    let active_id = use_memo(move || sessions.read().active_id.clone());

    let active_title = use_memo({
        let fallback = config.title.clone();
        move || {
            sessions
                .read()
                .active_id
                .as_ref()
                .and_then(|id| {
                    sessions
                        .read()
                        .sessions
                        .iter()
                        .find(|s| s.id == *id)
                        .map(|s| s.title.clone())
                })
                .unwrap_or_else(|| fallback.clone())
        }
    });

    AgentChatController {
        sessions,
        is_loading,
        input,
        messages,
        history,
        active_id,
        active_title,
        config,
    }
}

impl AgentChatController {
    /// Start a new empty chat session and make it active.
    pub fn new_chat(&self) {
        let id = new_id();
        let session = ChatSession {
            id: id.clone(),
            title: "New chat".to_string(),
            messages: vec![],
        };
        let mut sessions = self.sessions;
        sessions.with_mut(|s| {
            s.sessions.insert(0, session);
            s.active_id = Some(id);
        });
        self.save();
    }

    /// Make an existing session active.
    pub fn select_chat(&self, id: String) {
        let mut sessions = self.sessions;
        sessions.with_mut(|s| s.active_id = Some(id));
        self.save();
    }

    /// Delete a session, falling back to the first remaining one.
    pub fn delete_chat(&self, id: String) {
        let mut sessions = self.sessions;
        sessions.with_mut(|s| {
            s.sessions.retain(|session| session.id != id);
            if s.active_id.as_deref() == Some(id.as_str()) {
                s.active_id = s.sessions.first().map(|session| session.id.clone());
            }
        });
        self.save();
    }

    /// Send a question to the configured SSE endpoint and stream the answer
    /// into the active session (creating one on the first message).
    pub fn send(&self, question: String) {
        if self.sessions.read().active_id.is_none() {
            self.new_chat();
        }

        // Update the session title on the first user message and append the
        // new exchange.
        let mut sessions = self.sessions;
        sessions.with_mut(|s| {
            if let Some(id) = s.active_id.clone() {
                if let Some(session) = s.sessions.iter_mut().find(|s| s.id == id) {
                    let is_first_user = session
                        .messages
                        .iter()
                        .all(|m| !matches!(m, ChatMessageItem::User { .. }));
                    if is_first_user {
                        session.title = make_title(&question);
                    }
                    session.messages.push(ChatMessageItem::User {
                        content: question.clone(),
                    });
                    session.messages.push(ChatMessageItem::Assistant {
                        content: String::new(),
                        thinking: String::new(),
                        is_streaming: true,
                        tool_calls: vec![],
                    });
                }
            }
        });
        self.save();

        let mut is_loading = self.is_loading;
        is_loading.set(true);

        let config = self.config.clone();
        let sessions = self.sessions;
        spawn(async move {
            let mut on_event = move |event: AgentStreamEvent| {
                let mut sessions = sessions;
                sessions.with_mut(|s| {
                    if let Some(id) = s.active_id.clone() {
                        if let Some(session) = s.sessions.iter_mut().find(|s| s.id == id) {
                            reduce_event(session, event, &mut is_loading);
                        }
                    }
                });
            };

            if let Err(e) = stream_agent_events(&config.stream_url, &question, &mut on_event).await
            {
                let mut sessions = sessions;
                sessions.with_mut(|s| {
                    if let Some(id) = s.active_id.clone() {
                        if let Some(session) = s.sessions.iter_mut().find(|s| s.id == id) {
                            if let Some(ChatMessageItem::Assistant {
                                content,
                                is_streaming,
                                ..
                            }) = session.messages.last_mut()
                            {
                                *is_streaming = false;
                                content.push_str(&format!("\n\n[error: {e}]"));
                            }
                        }
                    }
                });
                is_loading.set(false);
            }
            save_sessions(config.storage_key.as_deref(), &sessions.read());
        });
    }

    fn save(&self) {
        save_sessions(self.config.storage_key.as_deref(), &self.sessions.read());
    }
}

/// Apply one streamed event to the in-progress assistant exchange.
fn reduce_event(session: &mut ChatSession, event: AgentStreamEvent, is_loading: &mut Signal<bool>) {
    match event {
        AgentStreamEvent::TextDelta { delta } => {
            if let Some(ChatMessageItem::Assistant { content, .. }) = session.messages.last_mut() {
                content.push_str(&delta);
            }
        }
        AgentStreamEvent::ThinkingDelta { delta } => {
            if let Some(ChatMessageItem::Assistant { thinking, .. }) = session.messages.last_mut() {
                thinking.push_str(&delta);
            }
        }
        AgentStreamEvent::ToolCall { name, .. } => {
            if let Some(ChatMessageItem::Assistant { tool_calls, .. }) = session.messages.last_mut()
            {
                tool_calls.push(ToolCall {
                    name,
                    result: None,
                    is_error: false,
                });
            }
        }
        AgentStreamEvent::ToolResult { output, is_error } => {
            if let Some(ChatMessageItem::Assistant { tool_calls, .. }) = session.messages.last_mut()
            {
                if let Some(call) = tool_calls.iter_mut().rev().find(|c| c.result.is_none()) {
                    call.result = Some(output);
                    call.is_error = is_error;
                }
            }
        }
        AgentStreamEvent::Done => {
            if let Some(ChatMessageItem::Assistant { is_streaming, .. }) =
                session.messages.last_mut()
            {
                *is_streaming = false;
            }
            is_loading.set(false);
        }
    }
}

fn storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load_sessions(key: &str) -> AgentChatSessions {
    if let Some(storage) = storage() {
        if let Ok(Some(raw)) = storage.get_item(key) {
            if let Ok(mut sessions) = serde_json::from_str::<AgentChatSessions>(&raw) {
                sessions.normalize_after_load();
                return sessions;
            }
        }
    }
    AgentChatSessions::default()
}

fn save_sessions(key: Option<&str>, sessions: &AgentChatSessions) {
    let Some(key) = key else { return };
    if let Some(storage) = storage() {
        if let Ok(raw) = serde_json::to_string(sessions) {
            let _ = storage.set_item(key, &raw);
        }
    }
}

fn new_id() -> String {
    let random = js_sys::Math::random().to_string();
    format!("chat-{random}")
}

fn make_title(text: &str) -> String {
    let trimmed = text.trim();
    let mut title: String = trimmed.chars().take(30).collect();
    if trimmed.chars().count() > 30 {
        title.push('…');
    }
    if title.is_empty() {
        title = "New chat".to_string();
    }
    title
}
