use dioxus::prelude::*;

use super::history::ChatHistory;
use super::input::ChatInputArea;
use super::types::ChatMessageItem;

/// A complete chat view: a centered, auto-scrolling transcript plus a
/// bottom composer, like kimi.com/chat.
#[component]
pub fn Chat(
    messages: ReadSignal<Vec<ChatMessageItem>>,
    input: Signal<String>,
    on_send: EventHandler<String>,
    #[props(default = false)] disabled: bool,
    #[props(default = "Ask anything...".to_string())] placeholder: String,
) -> Element {
    rsx! {
        div { class: "flex h-full flex-col bg-neutral-950 text-neutral-100",
            ChatHistory { messages }
            div { class: "shrink-0 px-4 pb-5 pt-2",
                div { class: "mx-auto max-w-3xl",
                    ChatInputArea {
                        input,
                        on_send,
                        disabled,
                        placeholder,
                    }
                }
            }
        }
    }
}
