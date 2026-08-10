use dioxus::prelude::*;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};

/// A single item in a chat history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatMessageItem {
    /// Message sent by the user.
    User { content: String },
    /// Message produced by the assistant. `is_streaming` is true while the
    /// response is still being generated.
    Assistant {
        content: String,
        is_streaming: bool,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
    },
}

/// A tool invocation recorded inside an assistant message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

/// A summary of a chat session shown in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatHistoryItem {
    pub id: String,
    pub title: String,
}

/// Render markdown text as HTML inside a styled container.
#[component]
pub fn Markdown(text: String) -> Element {
    let html_text = use_memo(move || markdown_to_html(&text));
    rsx! {
        div {
            class: "markdown text-neutral-100 leading-relaxed",
            dangerous_inner_html: "{html_text}",
        }
    }
}

fn markdown_to_html(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, options);
    let mut output = String::new();
    html::push_html(&mut output, parser);
    output
}

/// A complete chat panel: scrollable message list plus an input bar.
#[component]
pub fn Chat(
    messages: ReadSignal<Vec<ChatMessageItem>>,
    input: Signal<String>,
    on_send: EventHandler<String>,
    #[props(default = false)] is_loading: bool,
    #[props(default = false)] disabled: bool,
    #[props(default = "Ask anything...".to_string())] placeholder: String,
) -> Element {
    rsx! {
        div { class: "flex flex-col h-full bg-neutral-950 text-neutral-100",
            ChatHistory { messages, is_loading }
            ChatInputArea {
                input,
                on_send,
                disabled,
                placeholder,
            }
        }
    }
}

/// Scrollable list of chat messages.
#[component]
pub fn ChatHistory(
    messages: ReadSignal<Vec<ChatMessageItem>>,
    #[props(default = false)] is_loading: bool,
) -> Element {
    rsx! {
        div { class: "flex-1 overflow-y-auto px-4 py-6 space-y-6",
            for msg in messages.read().iter().cloned() {
                ChatMessage { message: msg }
            }
            if is_loading {
                StreamingIndicator {}
            }
        }
    }
}

/// A single chat message, styled by role (user vs assistant).
#[component]
pub fn ChatMessage(message: ChatMessageItem) -> Element {
    match message {
        ChatMessageItem::User { content } => rsx! {
            div { class: "flex justify-end",
                div { class: "max-w-[85%] rounded-2xl rounded-br-md px-4 py-2.5 bg-blue-600 text-white",
                    "{content}"
                }
            }
        },
        ChatMessageItem::Assistant {
            content,
            is_streaming,
            tool_calls,
        } => rsx! {
            div { class: "flex justify-start",
                div { class: "flex gap-3 max-w-[90%]",
                    div { class: "w-8 h-8 shrink-0 rounded-full bg-neutral-700 flex items-center justify-center text-xs font-medium text-neutral-300",
                        "AI"
                    }
                    div { class: "flex flex-col gap-2 min-w-0",
                        div { class: "rounded-2xl rounded-bl-md px-4 py-2.5 bg-neutral-800 text-neutral-100",
                            Markdown { text: content.clone() }
                            if is_streaming {
                                span { class: "inline-block w-0.5 h-4 ml-1 align-middle bg-blue-400 animate-pulse" }
                            }
                        }
                        if !tool_calls.is_empty() {
                            div { class: "flex flex-wrap gap-2",
                                for call in tool_calls.iter().cloned() {
                                    ToolCallBadge { call }
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

/// Small expandable badge for a tool call result.
#[component]
fn ToolCallBadge(call: ToolCall) -> Element {
    let mut expanded = use_signal(|| false);
    let status = if call.is_error {
        "text-red-400 border-red-900/50 bg-red-950/30"
    } else if call.result.is_some() {
        "text-emerald-400 border-emerald-900/50 bg-emerald-950/30"
    } else {
        "text-neutral-400 border-neutral-700 bg-neutral-900"
    };
    let label = if call.result.is_some() {
        format!("✓ {}", call.name)
    } else {
        format!("⟳ {}", call.name)
    };

    rsx! {
        div { class: "flex flex-col gap-1",
            button {
                class: "text-xs px-2 py-1 rounded border {status} hover:brightness-110 transition",
                onclick: move |_| expanded.set(!expanded()),
                "{label}"
            }
            if expanded() {
                if let Some(result) = call.result.as_ref() {
                    pre { class: "max-w-full overflow-x-auto rounded bg-neutral-950 border border-neutral-800 p-2 text-xs text-neutral-300",
                        code { "{result}" }
                    }
                }
            }
        }
    }
}

/// A narrow sidebar with a "New Chat" button and a list of recent chats.
#[component]
pub fn ChatSidebar(
    items: ReadSignal<Vec<ChatHistoryItem>>,
    active_id: Option<String>,
    on_new_chat: EventHandler,
    on_select: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "flex flex-col h-full w-64 bg-neutral-900 border-r border-neutral-800 text-neutral-100 shrink-0",
            div { class: "p-3",
                button {
                    class: "w-full flex items-center justify-center gap-2 rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm font-medium text-neutral-100 hover:bg-neutral-700 transition-colors",
                    onclick: move |_| on_new_chat.call(()),
                    span { class: "text-lg leading-none", "+" }
                    "New Chat"
                }
            }
            div { class: "flex-1 overflow-y-auto px-3 pb-3",
                if !items.read().is_empty() {
                    div { class: "mb-2 px-3 text-xs font-semibold uppercase tracking-wider text-neutral-500",
                        "History"
                    }
                }
                div { class: "space-y-1",
                    for item in items.read().iter().cloned() {
                        ChatSidebarItem {
                            id: item.id.clone(),
                            title: item.title.clone(),
                            active: active_id.as_ref() == Some(&item.id),
                            on_select,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ChatSidebarItem(
    id: String,
    title: String,
    active: bool,
    on_select: EventHandler<String>,
) -> Element {
    let active_class = if active {
        "bg-neutral-800 text-white"
    } else {
        "text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200"
    };
    rsx! {
        button {
            class: "w-full text-left rounded-lg px-3 py-2 text-sm truncate transition-colors {active_class}",
            onclick: move |_| on_select.call(id.clone()),
            title: "{title}",
            "{title}"
        }
    }
}

/// A centered welcome screen with a large input and optional suggestions.
#[component]
pub fn ChatWelcome(
    input: Signal<String>,
    on_send: EventHandler<String>,
    #[props(default = false)] disabled: bool,
    #[props(default = "Ask anything...".to_string())] placeholder: String,
    #[props(default = "What can I help with?".to_string())] title: String,
    #[props(default)] subtitle: Option<String>,
    #[props(default = Vec::new())] suggestions: Vec<String>,
) -> Element {
    rsx! {
        div { class: "flex flex-col h-full items-center justify-center bg-neutral-950 text-neutral-100 px-4",
            div { class: "w-full max-w-2xl text-center",
                h1 { class: "text-3xl font-semibold text-white mb-2", "{title}" }
                if let Some(subtitle) = subtitle {
                    p { class: "text-neutral-400 mb-10", "{subtitle}" }
                }
                ChatInputArea {
                    input,
                    on_send,
                    disabled,
                    placeholder,
                    is_welcome: true,
                }
                if !suggestions.is_empty() {
                    div { class: "mt-6 flex flex-wrap justify-center gap-2",
                        for suggestion in suggestions.iter().cloned() {
                            SuggestionChip {
                                text: suggestion.clone(),
                                on_click: on_send,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SuggestionChip(text: String, on_click: EventHandler<String>) -> Element {
    let text_for_click = text.clone();
    rsx! {
        button {
            class: "rounded-full border border-neutral-700 bg-neutral-900 px-4 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors",
            onclick: move |_| on_click.call(text_for_click.clone()),
            "{text}"
        }
    }
}

/// Bottom input bar with textarea and send button.
#[component]
pub fn ChatInputArea(
    input: Signal<String>,
    on_send: EventHandler<String>,
    #[props(default = false)] disabled: bool,
    #[props(default = "Ask anything...".to_string())] placeholder: String,
    #[props(default = false)] is_welcome: bool,
) -> Element {
    let mut submit = move || {
        let value = input.read().trim().to_string();
        if !value.is_empty() {
            on_send.call(value);
            input.set(String::new());
        }
    };

    let wrapper_class = if is_welcome {
        "rounded-2xl border border-neutral-700 bg-neutral-900 p-3 shadow-lg"
    } else {
        "border-t border-neutral-800 bg-neutral-900 p-4"
    };

    let textarea_class = if is_welcome {
        "flex-1 resize-none max-h-40 min-h-[72px] rounded-xl bg-neutral-800 border border-neutral-700 px-4 py-3 text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-blue-500 disabled:opacity-60"
    } else {
        "flex-1 resize-none max-h-40 min-h-[52px] rounded-xl bg-neutral-800 border border-neutral-700 px-4 py-3 text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-blue-500 disabled:opacity-60"
    };

    rsx! {
        div { class: "{wrapper_class}",
            div { class: "flex items-end gap-2 max-w-4xl mx-auto",
                textarea {
                    class: "{textarea_class}",
                    placeholder: "{placeholder}",
                    disabled: disabled,
                    rows: "1",
                    value: "{input.read()}",
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter && !e.modifiers().shift() {
                            e.prevent_default();
                            submit();
                        }
                    }
                }
                button {
                    class: "shrink-0 w-10 h-10 flex items-center justify-center rounded-xl bg-blue-600 hover:bg-blue-500 text-white disabled:opacity-50 disabled:cursor-not-allowed transition-colors",
                    disabled: disabled,
                    onclick: move |_| submit(),
                    "→"
                }
            }
        }
    }
}

/// Pulsing indicator shown while the assistant is thinking/responding.
#[component]
fn StreamingIndicator() -> Element {
    rsx! {
        div { class: "flex justify-start",
            div { class: "flex gap-3 max-w-[90%]",
                div { class: "w-8 h-8 shrink-0 rounded-full bg-neutral-700 flex items-center justify-center text-xs font-medium text-neutral-300",
                    "AI"
                }
                div { class: "rounded-2xl rounded-bl-md px-4 py-3 bg-neutral-800 text-neutral-100",
                    div { class: "flex gap-1.5",
                        span { class: "w-2 h-2 rounded-full bg-neutral-500 animate-bounce" }
                        span { class: "w-2 h-2 rounded-full bg-neutral-500 animate-bounce [animation-delay:0.2s]" }
                        span { class: "w-2 h-2 rounded-full bg-neutral-500 animate-bounce [animation-delay:0.4s]" }
                    }
                }
            }
        }
    }
}
