use dioxus::prelude::*;

use crate::types::{Action, StepResult, UnitKind};
use crate::utils::kind_label;

/// Ordered list of scheduled steps rendered as a todo list. Each row shows the
/// finish time on the left and a concise instruction like
/// "4 Eng T1 build Mex T2" on the right.
#[component]
pub fn StepTimeline(steps: Vec<StepResult>, on_click: EventHandler<UnitKind>) -> Element {
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if steps.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No steps in the schedule." }
            }
            div { class: "flex flex-col gap-1",
                for (idx, step) in steps.iter().enumerate() {
                    {
                        let (accent, description, clicked) = match &step.action {
                            Action::Build { target, builder } => (
                                "border-sky-700 hover:border-sky-600",
                                format!(
                                    "{} {} build {}",
                                    step.builder_count,
                                    kind_label(builder),
                                    kind_label(target),
                                ),
                                target.clone(),
                            ),
                            Action::Upgrade { from, to } => (
                                "border-amber-700 hover:border-amber-600",
                                format!(
                                    "{} {} upgrade to {}",
                                    step.builder_count,
                                    kind_label(from),
                                    kind_label(to),
                                ),
                                to.clone(),
                            ),
                        };
                        rsx! {
                            button {
                                class: "flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-900/60 hover:bg-neutral-800/80 transition-colors",
                                onclick: move |_| on_click.call(clicked.clone()),
                                span { class: "text-xs font-mono text-neutral-500 w-6 shrink-0 text-right", "#{idx + 1}" }
                                span { class: "text-xs font-mono text-sky-300 shrink-0", "t+{step.finish_time_seconds:.0}s" }
                                span { class: "flex-1 text-sm text-neutral-200 truncate", "{description}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
