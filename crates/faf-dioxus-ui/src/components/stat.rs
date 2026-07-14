use dioxus::prelude::*;

#[component]
pub fn Stat(label: String, value: Option<String>) -> Element {
    rsx! {
        div { class: "flex flex-col px-3 py-2 rounded bg-neutral-800/50 border border-neutral-800",
            span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
            span { class: "text-white font-medium",
                {value.as_deref().unwrap_or("—")}
            }
        }
    }
}
