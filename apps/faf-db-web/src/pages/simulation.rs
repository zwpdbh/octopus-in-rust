use dioxus::prelude::*;

use crate::types::ConstructionPlan;

#[component]
pub fn SimulationPage(plan: ConstructionPlan, on_back: EventHandler<()>) -> Element {
    rsx! {
        div {
            class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans overflow-hidden",
            header {
                class: "flex items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                button {
                    class: "px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    onclick: move |_| on_back.call(()),
                    "← Back to Simulate Build"
                }
                h1 { class: "text-lg font-semibold text-white", "Simulation" }
            }
            div {
                class: "flex-1 flex items-center justify-center p-8",
                div { class: "text-center space-y-3",
                    h2 { class: "text-xl font-semibold text-white", "Simulation Page" }
                    p { class: "text-neutral-400", "This page will run the construction simulation." }
                    p { class: "text-neutral-500 text-sm", "Queued items: {plan.items.len()}" }
                }
            }
        }
    }
}
