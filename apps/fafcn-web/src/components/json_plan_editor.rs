use dioxus::prelude::*;
use faf_blueprints::ConstructionPlan;

#[component]
pub fn JsonPlanEditor(plan: Signal<ConstructionPlan>) -> Element {
    let mut json_text = use_signal(|| serialize_plan(&plan.read()));
    let mut error = use_signal(|| String::new());
    let mut copied = use_signal(|| false);

    use_effect(move || {
        json_text.set(serialize_plan(&plan.read()));
    });

    rsx! {
        div { class: "flex-1 flex flex-col min-h-0 gap-3",
            div { class: "flex items-center gap-2 shrink-0",
                span { class: "text-xs text-neutral-400", "Edit the plan JSON below." }
                button {
                    class: "px-2 py-1 text-xs rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors shadow-sm",
                    onclick: move |_| {
                        let text = json_text.read().clone();
                        copy_to_clipboard(&text);
                        copied.set(true);
                    },
                    if *copied.read() { "Copied!" } else { "Copy" }
                }
            }
            textarea {
                class: "flex-1 min-h-0 w-full p-3 rounded bg-neutral-950 border border-neutral-700 text-xs font-mono text-neutral-300 resize-none focus:outline-none focus:border-blue-500",
                value: "{json_text}",
                oninput: move |e| {
                    copied.set(false);
                    let text = e.value();
                    json_text.set(text.clone());
                    match serde_json::from_str::<ConstructionPlan>(&text) {
                        Ok(parsed) => {
                            plan.set(parsed);
                            error.set(String::new());
                        }
                        Err(err) => {
                            error.set(format!("Invalid JSON: {err}"));
                        }
                    }
                },
            }
            if !error.read().is_empty() {
                p { class: "text-xs text-red-400 shrink-0", "{error}" }
            }
        }
    }
}

fn serialize_plan(plan: &ConstructionPlan) -> String {
    serde_json::to_string_pretty(plan).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let clipboard = window.navigator().clipboard();
        let _ = clipboard.write_text(text);
    }
}
