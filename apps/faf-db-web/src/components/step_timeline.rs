use dioxus::prelude::*;

use crate::components::EcoSnapshotView;
use crate::types::{Action, EcoSnapshot, StepReasoning, StepResult, UnitKind};
use crate::utils::kind_label;

/// Ordered list of scheduled steps rendered as a todo list. Each row shows the
/// finish time on the left and a concise instruction like
/// "4 Eng T1 build Mex T2" on the right. Clicking a row toggles an inline
/// details section that shows the economy state before the decision and the
/// top candidate actions considered.
#[component]
pub fn StepTimeline(
    steps: Vec<StepResult>,
    reasoning: Vec<StepReasoning>,
    initial_eco: EcoSnapshot,
    mut selected_step: Signal<Option<usize>>,
) -> Element {
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if steps.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No steps in the schedule." }
            }
            div { class: "flex flex-col gap-1",
                for (idx, step) in steps.iter().enumerate() {
                    {
                        let is_selected = *selected_step.read() == Some(idx);
                        let accent = match &step.action {
                            Action::Build { .. } => "border-sky-700 hover:border-sky-600",
                            Action::Upgrade { .. } => "border-amber-700 hover:border-amber-600",
                        };
                        let description = describe_step(&step.action);
                        let row_class = if is_selected {
                            format!("flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-800 transition-colors")
                        } else {
                            format!("flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-900/60 hover:bg-neutral-800/80 transition-colors")
                        };
                        rsx! {
                            div { class: "flex flex-col",
                                button {
                                    class: "{row_class}",
                                    onclick: move |_| {
                                        if is_selected {
                                            selected_step.set(None);
                                        } else {
                                            selected_step.set(Some(idx));
                                        }
                                    },
                                    span { class: "text-xs font-mono text-neutral-500 w-6 shrink-0 text-right", "#{idx + 1}" }
                                    span { class: "text-xs font-mono text-sky-300 shrink-0", "t+{step.finish_time_seconds:.0}s" }
                                    span { class: "flex-1 text-sm text-neutral-200 truncate", "{description}" }
                                }
                                if is_selected {
                                    StepDetails {
                                        step: step.clone(),
                                        reasoning: reasoning.get(idx).cloned(),
                                        pre_eco: pre_step_eco(&steps, &initial_eco, idx),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn StepDetails(
    step: StepResult,
    reasoning: Option<StepReasoning>,
    pre_eco: EcoSnapshot,
) -> Element {
    rsx! {
        div { class: "mt-1 ml-6 rounded border border-neutral-700 bg-neutral-950/60 p-3 flex flex-col lg:flex-row gap-4",
            // Economy snapshot before the decision.
            div { class: "flex flex-col gap-1 lg:w-96 shrink-0",
                h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide", "Economy before decision" }
                EcoSnapshotView { snapshot: pre_eco }
            }

            // Candidate reasoning.
            div { class: "flex flex-col gap-1 flex-1 min-w-0",
                h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide", "Top candidates" }
                if let Some(reasoning) = reasoning {
                    if reasoning.top_candidates.is_empty() {
                        p { class: "text-xs text-neutral-500", "No candidate data recorded." }
                    } else {
                        for candidate in reasoning.top_candidates.iter() {
                            {
                                let is_chosen = candidate.action == step.action;
                                let row_class = if is_chosen {
                                    "flex items-center gap-2 px-2 py-1 rounded bg-emerald-900/30 border border-emerald-700/50"
                                } else {
                                    "flex items-center gap-2 px-2 py-1 rounded bg-neutral-900/50 border border-neutral-800"
                                };
                                let score_class = if is_chosen { "text-xs font-mono text-emerald-300 w-12 text-right" } else { "text-xs font-mono text-neutral-400 w-12 text-right" };
                                rsx! {
                                    div { class: "{row_class}",
                                        span { class: "{score_class}", "{candidate.score:.1}" }
                                        span { class: "text-xs text-neutral-200 truncate", "{describe_step(&candidate.action)}" }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    p { class: "text-xs text-neutral-500", "No reasoning available for this step." }
                }
            }
        }
    }
}


fn pre_step_eco(steps: &[StepResult], initial_eco: &EcoSnapshot, idx: usize) -> EcoSnapshot {
    if idx == 0 {
        initial_eco.clone()
    } else {
        steps
            .get(idx - 1)
            .map(|s| s.economy.clone())
            .unwrap_or_else(|| initial_eco.clone())
    }
}

fn describe_step(action: &Action) -> String {
    match action {
        Action::Build { target, builder } => {
            let builder_text = describe_builders(builder);
            format!("{} build {}", builder_text, kind_label(target))
        }
        Action::Upgrade { from, to, assisted_by } => {
            let assist_text = if assisted_by.is_empty() {
                String::new()
            } else {
                format!(" (assisted by {})", describe_builders(assisted_by))
            };
            format!("{} upgrade to {}{}", kind_label(from), kind_label(to), assist_text)
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
