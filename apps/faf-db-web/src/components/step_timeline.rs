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
                            Action::Build { target, builder } => {
                                let builder_text = describe_builders(builder);
                                (
                                    "border-sky-700 hover:border-sky-600",
                                    format!("{} build {}", builder_text, kind_label(target)),
                                    target.clone(),
                                )
                            }
                            Action::Upgrade { from, to, assisted_by } => {
                                let assist_text = if assisted_by.is_empty() {
                                    String::new()
                                } else {
                                    format!(" (assisted by {})", describe_builders(assisted_by))
                                };
                                (
                                    "border-amber-700 hover:border-amber-600",
                                    format!(
                                        "{} upgrade to {}{}",
                                        kind_label(from),
                                        kind_label(to),
                                        assist_text,
                                    ),
                                    to.clone(),
                                )
                            }
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

/// Summarise a list of builders as a count + kind string. Identical consecutive
/// kinds are collapsed; mixed kinds are listed with counts.
fn describe_builders(builders: &[UnitKind]) -> String {
    if builders.is_empty() {
        return "0 builders".to_string();
    }
    let mut groups: Vec<(UnitKind, usize)> = Vec::new();
    for b in builders {
        if let Some((last_kind, count)) = groups.last_mut() {
            if last_kind == b {
                *count += 1;
                continue;
            }
        }
        groups.push((b.clone(), 1));
    }
    groups
        .iter()
        .map(|(kind, count)| format!("{} {}", count, kind_label(kind)))
        .collect::<Vec<_>>()
        .join(" + ")
}
