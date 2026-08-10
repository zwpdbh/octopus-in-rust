use dioxus::prelude::*;
use faf_dioxus_ui::components::chat::{
    Chat, ChatHistoryItem, ChatMessageItem, ChatSidebar, ChatWelcome, ToolCall,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response, Storage};

const ASK_STREAM_URL: &str = "http://localhost:3000/api/ask/stream";
const STORAGE_KEY: &str = "fafcn_qa_state";

/// Persisted chat session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ChatSession {
    id: String,
    title: String,
    messages: Vec<ChatMessageItem>,
}

/// All Q&A state stored in localStorage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct QaState {
    sessions: Vec<ChatSession>,
    active_id: Option<String>,
}

/// Events emitted by the server's `POST /api/ask/stream` SSE endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
enum QaStreamEvent {
    TextDelta {
        delta: String,
    },
    ThinkingDelta {
        delta: String,
    },
    ToolCall {
        name: String,
        #[allow(dead_code)]
        arguments: serde_json::Value,
    },
    ToolResult {
        output: String,
        #[allow(dead_code)]
        is_error: bool,
    },
    Done,
}

#[component]
pub fn Qa() -> Element {
    let mut state = use_signal(load_state);
    let mut is_loading = use_signal(|| false);
    let input = use_signal(String::new);

    // Derived view of the currently active session's messages.
    let messages = use_memo(move || {
        state
            .read()
            .active_id
            .as_ref()
            .and_then(|id| {
                state
                    .read()
                    .sessions
                    .iter()
                    .find(|s| s.id == *id)
                    .map(|s| s.messages.clone())
            })
            .unwrap_or_default()
    });

    let history = use_memo(move || {
        state
            .read()
            .sessions
            .iter()
            .map(|s| ChatHistoryItem {
                id: s.id.clone(),
                title: s.title.clone(),
            })
            .collect::<Vec<_>>()
    });

    let active_id = use_memo(move || state.read().active_id.clone());

    let active_title = use_memo(move || {
        state
            .read()
            .active_id
            .as_ref()
            .and_then(|id| {
                state
                    .read()
                    .sessions
                    .iter()
                    .find(|s| s.id == *id)
                    .map(|s| s.title.clone())
            })
            .unwrap_or_else(|| "FAF Q&A".to_string())
    });

    let mut create_new_chat = move || {
        let id = new_id();
        let session = ChatSession {
            id: id.clone(),
            title: "New chat".to_string(),
            messages: vec![],
        };
        state.with_mut(|s| {
            s.sessions.insert(0, session);
            s.active_id = Some(id);
        });
        save_state(&state.read());
    };

    let select_chat = move |id: String| {
        state.with_mut(|s| s.active_id = Some(id));
        save_state(&state.read());
    };

    let on_send = move |question: String| {
        // Create a session if this is the first message.
        if state.read().active_id.is_none() {
            create_new_chat();
        }

        // Update the session title on the first user message and append the
        // new exchange.
        state.with_mut(|s| {
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
                        is_streaming: true,
                        tool_calls: vec![],
                    });
                }
            }
        });
        save_state(&state.read());
        is_loading.set(true);

        spawn(async move {
            let mut on_event = move |event: QaStreamEvent| {
                state.with_mut(|s| {
                    if let Some(id) = s.active_id.clone() {
                        if let Some(session) = s.sessions.iter_mut().find(|s| s.id == id) {
                            match event {
                                QaStreamEvent::TextDelta { delta }
                                | QaStreamEvent::ThinkingDelta { delta } => {
                                    if let Some(ChatMessageItem::Assistant { content, .. }) =
                                        session.messages.last_mut()
                                    {
                                        content.push_str(&delta);
                                    }
                                }
                                QaStreamEvent::ToolCall { name, .. } => {
                                    if let Some(ChatMessageItem::Assistant { tool_calls, .. }) =
                                        session.messages.last_mut()
                                    {
                                        tool_calls.push(ToolCall {
                                            name,
                                            result: None,
                                            is_error: false,
                                        });
                                    }
                                }
                                QaStreamEvent::ToolResult { output, is_error } => {
                                    if let Some(ChatMessageItem::Assistant { tool_calls, .. }) =
                                        session.messages.last_mut()
                                    {
                                        if let Some(call) =
                                            tool_calls.iter_mut().rev().find(|c| c.result.is_none())
                                        {
                                            call.result = Some(output);
                                            call.is_error = is_error;
                                        }
                                    }
                                }
                                QaStreamEvent::Done => {
                                    if let Some(ChatMessageItem::Assistant {
                                        is_streaming, ..
                                    }) = session.messages.last_mut()
                                    {
                                        *is_streaming = false;
                                    }
                                    is_loading.set(false);
                                }
                            }
                        }
                    }
                });
            };

            if let Err(e) = stream_ask(&question, &mut on_event).await {
                state.with_mut(|s| {
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
            save_state(&state.read());
        });
    };

    rsx! {
        div { class: "flex h-full bg-neutral-950",
            ChatSidebar {
                items: history,
                active_id: active_id(),
                on_new_chat: move |_| create_new_chat(),
                on_select: select_chat,
            }
            div { class: "flex-1 min-w-0 flex flex-col",
                if messages.read().is_empty() {
                    ChatWelcome {
                        input,
                        on_send,
                        disabled: *is_loading.read(),
                        placeholder: "Ask about a unit, build order, or economy...".to_string(),
                        title: "FAF Q&A".to_string(),
                        subtitle: Some("Ask anything about Forged Alliance Forever units and economy.".to_string()),
                        suggestions: vec![
                            "Explain the Cybran Monkey Lord".to_string(),
                            "What is a good build order for UEF?".to_string(),
                            "How do mass extractors work?".to_string(),
                        ],
                    }
                } else {
                    div { class: "px-4 py-3 bg-neutral-900 border-b border-neutral-800 shrink-0",
                        h2 { class: "text-sm font-medium text-neutral-200 truncate", "{active_title}" }
                    }
                    Chat {
                        messages,
                        input,
                        on_send,
                        is_loading: false,
                        disabled: *is_loading.read(),
                        placeholder: "Ask about a unit, build order, or economy...".to_string(),
                    }
                }
            }
        }
    }
}

/// Open a streaming POST request to the Q&A endpoint and parse SSE chunks.
async fn stream_ask(
    question: &str,
    on_event: &mut impl FnMut(QaStreamEvent),
) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;

    let body = serde_json::json!({ "question": question }).to_string();

    let headers = Headers::new().map_err(|e| format!("headers: {e:?}"))?;
    headers
        .append("Content-Type", "application/json")
        .map_err(|e| format!("headers: {e:?}"))?;

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&body));
    init.set_headers(&headers);

    let request = Request::new_with_str_and_init(ASK_STREAM_URL, &init)
        .map_err(|e| format!("request: {e:?}"))?;

    let response_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let response: Response = response_value
        .dyn_into()
        .map_err(|e| format!("response cast: {e:?}"))?;

    if !response.ok() {
        let status = response.status();
        return Err(format!("HTTP {status}"));
    }

    let stream = response.body().ok_or("response has no body")?;
    let reader = stream
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|e| format!("reader cast: {e:?}"))?;

    let mut buffer = String::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|e| format!("read: {e:?}"))?;

        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(|e| format!("reflect done: {e:?}"))?
            .as_bool()
            .unwrap_or(true);

        if done {
            break;
        }

        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|e| format!("reflect value: {e:?}"))?;
        let chunk = js_sys::Uint8Array::from(value).to_vec();
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((event_text, rest)) = buffer.split_once("\n\n") {
            parse_sse_event(event_text, on_event);
            buffer = rest.to_string();
        }
    }

    if !buffer.is_empty() {
        parse_sse_event(&buffer, on_event);
    }

    Ok(String::new())
}

fn parse_sse_event(text: &str, on_event: &mut impl FnMut(QaStreamEvent)) {
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim_start();
            if data.is_empty() {
                continue;
            }
            match serde_json::from_str::<QaStreamEvent>(data) {
                Ok(event) => on_event(event),
                Err(e) => {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "failed to parse SSE event: {e}: {data}"
                    )));
                }
            }
        }
    }
}

fn storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load_state() -> QaState {
    if let Some(storage) = storage() {
        if let Ok(Some(raw)) = storage.get_item(STORAGE_KEY) {
            if let Ok(state) = serde_json::from_str::<QaState>(&raw) {
                return state;
            }
        }
    }
    QaState::default()
}

fn save_state(state: &QaState) {
    if let Some(storage) = storage() {
        if let Ok(raw) = serde_json::to_string(state) {
            let _ = storage.set_item(STORAGE_KEY, &raw);
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
