use dioxus::prelude::*;

#[component]
pub fn PlanEditor(plan_json: Signal<String>, error: Signal<Option<String>>) -> Element {
    let border_class = if error.read().is_some() {
        "border-red-600 focus:border-red-500"
    } else {
        "border-neutral-700 focus:border-blue-500"
    };

    rsx! {
        div { class: "flex flex-col gap-2",
            label { class: "text-sm font-medium text-neutral-300", "Construction Plan (JSON)" }
            textarea {
                class: "w-full h-48 bg-neutral-900 border {border_class} rounded-lg p-3 text-xs font-mono text-gray-200 resize-none focus:outline-none",
                value: "{plan_json}",
                oninput: move |evt| plan_json.set(evt.value()),
            }
            if let Some(err) = error.read().as_ref() {
                div { class: "text-xs text-red-400", "{err}" }
            }
        }
    }
}
