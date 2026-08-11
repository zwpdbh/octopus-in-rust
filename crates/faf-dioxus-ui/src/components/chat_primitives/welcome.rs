use dioxus::prelude::*;

use super::input::ChatInputArea;

/// Centered hero shown before the first message: a big greeting, the large
/// composer and optional suggestion chips, like kimi.com/chat.
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
        div { class: "flex h-full flex-col items-center justify-center bg-neutral-950 px-4 text-neutral-100",
            div { class: "flex w-full max-w-3xl flex-col",
                h1 { class: "text-center text-3xl font-medium tracking-tight text-neutral-100 sm:text-4xl",
                    "{title}"
                }
                if let Some(subtitle) = subtitle {
                    p { class: "mt-3 text-center text-[15px] text-neutral-400", "{subtitle}" }
                }
                div { class: "mt-10",
                    ChatInputArea {
                        input,
                        on_send,
                        disabled,
                        placeholder,
                    }
                }
                if !suggestions.is_empty() {
                    div { class: "mt-6 flex flex-wrap justify-center gap-2",
                        for suggestion in suggestions.iter().cloned() {
                            SuggestionChip {
                                text: suggestion,
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
            class: "rounded-full border border-neutral-700/80 bg-neutral-900/60 px-4 py-1.5 text-sm text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors cursor-pointer",
            onclick: move |_| on_click.call(text_for_click.clone()),
            "{text}"
        }
    }
}
