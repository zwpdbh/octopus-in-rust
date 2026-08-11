use dioxus::prelude::*;

/// Kimi-style composer: a single rounded, elevated box containing an
/// auto-growing textarea and a circular send button. Enter sends,
/// Shift+Enter inserts a newline.
#[component]
pub fn ChatInputArea(
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

    let can_send = !disabled && !input.read().trim().is_empty();
    let send_class = if can_send {
        "bg-neutral-100 text-neutral-900 hover:bg-white cursor-pointer"
    } else {
        "bg-neutral-800 text-neutral-600 cursor-not-allowed"
    };

    rsx! {
        div { class: "rounded-[28px] border border-neutral-700/80 bg-neutral-900 shadow-lg shadow-black/20 transition-colors focus-within:border-neutral-500",
            textarea {
                class: "block w-full resize-none bg-transparent px-5 pt-4 pb-2 text-[15px] leading-relaxed text-neutral-100 placeholder:text-neutral-500 focus:outline-none field-sizing-content max-h-48 disabled:opacity-60",
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
                },
            }
            div { class: "flex items-center justify-end px-3 pb-3",
                button {
                    class: "flex h-9 w-9 items-center justify-center rounded-full transition-colors {send_class}",
                    disabled: !can_send,
                    aria_label: "Send",
                    onclick: move |_| submit(),
                    svg {
                        class: "h-4 w-4",
                        fill: "none",
                        stroke: "currentColor",
                        stroke_width: "2",
                        view_box: "0 0 24 24",
                        path {
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            d: "M12 19V5m-7 7 7-7 7 7",
                        }
                    }
                }
            }
        }
    }
}
