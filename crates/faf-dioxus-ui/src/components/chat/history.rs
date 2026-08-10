use dioxus::prelude::*;

use super::message::ChatMessage;
use super::types::ChatMessageItem;

/// Scrollable, centered message transcript that sticks to the bottom while
/// new content streams in.
#[component]
pub fn ChatHistory(messages: ReadSignal<Vec<ChatMessageItem>>) -> Element {
    let mut scroll_el = use_signal(|| None::<web_sys::Element>);

    // Keep the view pinned to the latest message on every transcript change.
    use_effect(move || {
        let _ = messages.read();
        if let Some(el) = scroll_el.read().as_ref() {
            el.set_scroll_top(el.scroll_height());
        }
    });

    rsx! {
        div {
            class: "flex-1 min-h-0 overflow-y-auto",
            onmounted: move |evt| {
                if let Some(el) = evt.data().downcast::<web_sys::Element>() {
                    scroll_el.set(Some(el.clone()));
                }
            },
            div { class: "mx-auto flex max-w-3xl flex-col gap-7 px-4 py-6",
                for msg in messages.read().iter().cloned() {
                    ChatMessage { message: msg }
                }
            }
        }
    }
}
