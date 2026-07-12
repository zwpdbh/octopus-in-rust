use dioxus::prelude::*;

#[component]
pub fn SliderField(
    label: String,
    value: f64,
    min: f64,
    max: f64,
    unit: String,
    on_change: EventHandler<f64>,
) -> Element {
    rsx! {
        div { class: "text-sm",
            div { class: "flex items-center justify-between",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
                span { class: "text-xs text-neutral-300", "{value:.0}{unit}" }
            }
            input {
                r#type: "range",
                min: "{min}",
                max: "{max}",
                value: "{value}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        on_change.call(v);
                    }
                },
                class: "w-full h-2 mt-1 bg-neutral-700 rounded-lg appearance-none cursor-pointer accent-blue-500",
            }
        }
    }
}
