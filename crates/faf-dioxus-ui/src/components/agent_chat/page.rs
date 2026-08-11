//! Batteries-included agent chat page component.

use dioxus::prelude::*;

use super::controller::use_agent_chat;
use super::state::AgentChatConfig;
use crate::components::chat_primitives::{Chat, ChatSidebar, ChatWelcome};

/// A complete agent chat page: session sidebar, welcome screen with
/// suggestions, streaming transcript, and composer.
///
/// Point it at any SSE endpoint that accepts `POST {"question": "..."}` and
/// streams [`super::AgentStreamEvent`] frames:
///
/// ```rust
/// // docref: demo
/// rsx! {
///     AgentChat {
///         config: AgentChatConfig {
///             stream_url: "http://localhost:3000/api/ask/stream".into(),
///             storage_key: Some("my_app_chat_state".into()),
///             ..Default::default()
///         },
///     }
/// }
/// ```
///
/// For a custom layout, build on [`super::use_agent_chat`] and the
/// [`crate::components::chat_primitives`] primitives instead.
#[component]
pub fn AgentChat(config: AgentChatConfig) -> Element {
    let controller = use_agent_chat(config.clone());

    let input = controller.input;
    let is_loading = controller.is_loading;
    let messages = controller.messages;
    let history = controller.history;
    let active_id = controller.active_id;
    let active_title = controller.active_title;

    let on_send = {
        let controller = controller.clone();
        move |question: String| controller.send(question)
    };
    let on_new_chat = {
        let controller = controller.clone();
        move |_| controller.new_chat()
    };
    let on_select = {
        let controller = controller.clone();
        move |id: String| controller.select_chat(id)
    };
    let on_delete = {
        let controller = controller.clone();
        move |id: String| controller.delete_chat(id)
    };

    rsx! {
        div { class: "flex h-full bg-neutral-950",
            ChatSidebar {
                items: history,
                active_id: active_id(),
                on_new_chat,
                on_select,
                on_delete,
            }
            div { class: "flex-1 min-w-0 flex flex-col",
                if messages.read().is_empty() {
                    ChatWelcome {
                        input,
                        on_send,
                        disabled: *is_loading.read(),
                        placeholder: config.placeholder.clone(),
                        title: config.title.clone(),
                        subtitle: config.subtitle.clone(),
                        suggestions: config.suggestions.clone(),
                    }
                } else {
                    div { class: "h-12 flex items-center justify-center px-4 border-b border-neutral-800/60 shrink-0",
                        h2 { class: "text-sm text-neutral-400 truncate", "{active_title}" }
                    }
                    Chat {
                        messages,
                        input,
                        on_send,
                        disabled: *is_loading.read(),
                        placeholder: config.placeholder.clone(),
                    }
                }
            }
        }
    }
}
