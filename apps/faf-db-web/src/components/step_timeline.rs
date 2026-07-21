use dioxus::prelude::*;

use crate::types::{Action, StepResult, UnitKind};
use crate::utils::kind_label;

/// Ordered list of scheduled steps. Clicking a step emits the clicked unit
/// kind so the caller can show its details.
#[component]
pub fn StepTimeline(steps: Vec<StepResult>, on_click: EventHandler<UnitKind>) -> Element {
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if steps.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No steps in the schedule." }
            }
            div { class: "flex flex-col gap-2",
                for (idx, step) in steps.iter().enumerate() {
                    {
                        let (icon, accent, text, clicked) = match &step.action {
                            Action::Build { target, builder } => (
                                "🔨",
                                "border-sky-700",
                                format!("Build {} w/ {}", kind_label(target), kind_label(builder)),
                                target.clone(),
                            ),
                            Action::Upgrade { from, to } => (
                                "⬆️",
                                "border-amber-700",
                                format!("Upgrade {}→{}", kind_label(from), kind_label(to)),
                                to.clone(),
                            ),
                        };
                        rsx! {
                            button {
                                class: "flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-900/60 hover:bg-neutral-800/80 transition-colors",
                                onclick: move |_| on_click.call(clicked.clone()),
                                span { class: "text-xs font-mono text-neutral-500 w-6 shrink-0 text-right", "#{idx + 1}" }
                                span { class: "text-base leading-none", "{icon}" }
                                span { class: "flex-1 text-sm text-neutral-200 truncate", "{text}" }
                                span { class: "text-xs font-mono text-neutral-400 shrink-0", "t={step.finish_time_seconds:.0}s" }
                            }
                        }
                    }
                }
            }
        }
    }
}
