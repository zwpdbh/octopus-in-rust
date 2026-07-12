use dioxus::prelude::*;

#[component]
pub fn CountSlider(value: u32, on_change: EventHandler<u32>) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            input {
                r#type: "range",
                min: "1",
                max: "10",
                value: "{value}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<u32>() {
                        on_change.call(v.clamp(1, 10));
                    }
                },
                class: "w-20 h-1.5 bg-neutral-700 rounded-lg appearance-none cursor-pointer accent-blue-500",
            }
            span { class: "text-xs text-neutral-300 w-4", "{value}" }
        }
    }
}
