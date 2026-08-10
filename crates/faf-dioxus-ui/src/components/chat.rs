use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

/// A single item in a chat history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatMessageItem {
    /// Message sent by the user.
    User { content: String },
    /// Message produced by the assistant. `is_streaming` is true while the
    /// response is still being generated.
    Assistant { content: String, is_streaming: bool },
}

/// A summary of a chat session shown in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatHistoryItem {
    pub id: String,
    pub title: String,
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
            div { class: "flex-1 overflow-y-auto px-4 py-6 space-y-5",
                for msg in messages.read().iter().cloned() {
                    ChatMessage { message: msg }
                }
                if is_loading {
                    StreamingIndicator {}
                }
            }
            ChatInput {
                input,
                on_send,
                disabled,
                placeholder,
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
        div { class: "flex flex-col h-full w-64 bg-neutral-900 border-r border-neutral-800 text-neutral-100",
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
    let mut submit = move || {
        let value = input.read().trim().to_string();
        if !value.is_empty() {
            on_send.call(value);
            input.set(String::new());
        }
    };

    rsx! {
        div { class: "flex flex-col h-full items-center justify-center bg-neutral-950 text-neutral-100 px-4",
            div { class: "w-full max-w-2xl text-center",
                h1 { class: "text-3xl font-semibold text-white mb-2", "{title}" }
                if let Some(subtitle) = subtitle {
                    p { class: "text-neutral-400 mb-10", "{subtitle}" }
                }
                div { class: "relative rounded-2xl border border-neutral-700 bg-neutral-900 p-3 shadow-lg",
                    textarea {
                        class: "w-full resize-none max-h-40 min-h-[72px] rounded-xl bg-neutral-800 border border-neutral-700 px-4 py-3 pr-14 text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500 disabled:opacity-60",
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
                        class: "absolute right-4 bottom-4 w-9 h-9 flex items-center justify-center rounded-lg bg-blue-600 hover:bg-blue-500 text-white disabled:opacity-50 disabled:cursor-not-allowed transition-colors",
                        disabled: disabled,
                        onclick: move |_| submit(),
                        "→"
                    }
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

/// Render one chat bubble.
#[component]
pub fn ChatMessage(message: ChatMessageItem) -> Element {
    match message {
        ChatMessageItem::User { content } => rsx! {
            div { class: "flex justify-end",
                div { class: "max-w-[80%] rounded-2xl rounded-br-md px-4 py-2.5 bg-blue-600 text-white whitespace-pre-wrap",
                    "{content}"
                }
            }
        },
        ChatMessageItem::Assistant {
            content,
            is_streaming,
        } => rsx! {
            div { class: "flex justify-start",
                div { class: "flex gap-3 max-w-[90%]",
                    div { class: "w-8 h-8 shrink-0 rounded-full bg-neutral-700 flex items-center justify-center text-xs font-medium text-neutral-300",
                        "AI"
                    }
                    div { class: "rounded-2xl rounded-bl-md px-4 py-2.5 bg-neutral-800 text-neutral-100 whitespace-pre-wrap",
                        "{content}"
                        if is_streaming {
                            span { class: "inline-block w-0.5 h-4 ml-1 align-middle bg-blue-400 animate-pulse" }
                        }
                    }
                }
            }
        },
    }
}

/// Bottom input bar with textarea and send button.
#[component]
pub fn ChatInput(
    input: Signal<String>,
    on_send: EventHandler<String>,
    #[props(default = false)] disabled: bool,
    #[props(default = "Ask anything...".to_string())] placeholder: String,
) -> Element {
    let mut submit = move || {
        let value = input.read().trim().to_string();
        if !value.is_empty() {
            on_send.call(value);
            input.set(String::new());
        }
    };

    rsx! {
        div { class: "border-t border-neutral-800 bg-neutral-900 p-4",
            div { class: "relative max-w-4xl mx-auto",
                textarea {
                    class: "w-full resize-none max-h-40 min-h-[52px] rounded-xl bg-neutral-800 border border-neutral-700 px-4 py-3 pr-12 text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-blue-500 disabled:opacity-60",
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
                    class: "absolute right-3 bottom-3 w-9 h-9 flex items-center justify-center rounded-lg bg-blue-600 hover:bg-blue-500 text-white disabled:opacity-50 disabled:cursor-not-allowed transition-colors",
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
