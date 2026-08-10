use dioxus::prelude::*;

use super::super::markdown::Markdown;
use super::types::{ChatMessageItem, ToolCall};

/// A single chat message, styled by role (user vs assistant).
///
/// User messages are right-aligned rounded bubbles; assistant messages are
/// full-width, bubble-less markdown, like kimi.com/chat.
#[component]
pub fn ChatMessage(message: ChatMessageItem) -> Element {
    match message {
        ChatMessageItem::User { content } => rsx! {
            div { class: "flex justify-end",
                div { class: "max-w-[75%] rounded-3xl bg-neutral-800 px-5 py-3 text-[15px] leading-relaxed text-neutral-100 whitespace-pre-wrap break-words",
                    "{content}"
                }
            }
        },
        ChatMessageItem::Assistant {
            content,
            thinking,
            is_streaming,
            tool_calls,
        } => {
            let show_typing =
                is_streaming && content.is_empty() && thinking.is_empty() && tool_calls.is_empty();
            let thinking_title = if is_streaming && content.is_empty() {
                "Thinking...".to_string()
            } else {
                "Thought process".to_string()
            };
            rsx! {
                div { class: "flex flex-col gap-3",
                    if !thinking.is_empty() {
                        CollapsibleBox {
                            title: thinking_title,
                            children: rsx! {
                                pre { class: "whitespace-pre-wrap text-sm text-neutral-400 leading-relaxed",
                                    "{thinking}"
                                }
                            }
                        }
                    }
                    if !tool_calls.is_empty() {
                        CollapsibleBox {
                            title: format!("Tool calls ({})", tool_calls.len()),
                            children: rsx! {
                                div { class: "flex flex-col gap-2",
                                    for call in tool_calls.iter().cloned() {
                                        ToolCallBadge { call }
                                    }
                                }
                            }
                        }
                    }
                    if show_typing {
                        TypingDots {}
                    } else {
                        div { class: "text-[15px] leading-7 text-neutral-200",
                            Markdown { text: content.clone() }
                            if is_streaming {
                                span { class: "ml-1 inline-block h-2.5 w-2.5 rounded-full bg-blue-400 align-middle animate-pulse" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A collapsible section for secondary content (thinking, tool calls),
/// rendered as a subtle header row with an indented thread line.
#[component]
fn CollapsibleBox(title: String, children: Element) -> Element {
    let mut expanded = use_signal(|| false);
    let chevron_class = if expanded() { "rotate-90" } else { "" };
    rsx! {
        div { class: "select-none",
            button {
                class: "flex items-center gap-1.5 text-sm text-neutral-500 hover:text-neutral-300 transition-colors cursor-pointer",
                onclick: move |_| expanded.set(!expanded()),
                span { class: "inline-block text-xs transition-transform {chevron_class}", "▶" }
                "{title}"
            }
            if expanded() {
                div { class: "mt-2 ml-1 border-l-2 border-neutral-800 pl-4",
                    {children}
                }
            }
        }
    }
}

/// Small expandable badge for a tool call result.
#[component]
fn ToolCallBadge(call: ToolCall) -> Element {
    let mut expanded = use_signal(|| false);
    let (dot_class, label) = if call.is_error {
        ("bg-red-400", call.name.clone())
    } else if call.result.is_some() {
        ("bg-emerald-400", call.name.clone())
    } else {
        ("bg-neutral-500 animate-pulse", call.name.clone())
    };

    rsx! {
        div { class: "flex flex-col gap-1 items-start",
            button {
                class: "flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900/60 px-3 py-1.5 text-xs text-neutral-300 hover:border-neutral-600 transition-colors cursor-pointer",
                onclick: move |_| expanded.set(!expanded()),
                span { class: "h-1.5 w-1.5 rounded-full {dot_class}" }
                "{label}"
            }
            if expanded() {
                if let Some(result) = call.result.as_ref() {
                    pre { class: "max-w-full overflow-x-auto rounded-lg bg-neutral-950 border border-neutral-800 p-3 text-xs text-neutral-300",
                        code { "{result}" }
                    }
                }
            }
        }
    }
}

/// Bouncing dots shown while waiting for the first assistant token.
#[component]
fn TypingDots() -> Element {
    rsx! {
        div { class: "flex items-center gap-1.5 py-2",
            span { class: "h-2 w-2 rounded-full bg-neutral-500 animate-bounce" }
            span { class: "h-2 w-2 rounded-full bg-neutral-500 animate-bounce [animation-delay:0.2s]" }
            span { class: "h-2 w-2 rounded-full bg-neutral-500 animate-bounce [animation-delay:0.4s]" }
        }
    }
}
