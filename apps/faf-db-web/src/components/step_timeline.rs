use dioxus::prelude::*;

use crate::components::EcoSnapshotView;
use crate::types::{Action, EcoSnapshot, StepReasoning, StepResult, UnitKind};
use crate::utils::kind_label;

/// Ordered list of scheduled steps rendered as a todo list. Each row shows the
/// start/end time on the left and a concise instruction like
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
                            format!("flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-800 transition-colors cursor-pointer")
                        } else {
                            format!("flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent} bg-neutral-900/60 hover:bg-neutral-800/80 transition-colors cursor-pointer")
                        };
                        let start_seconds = if idx == 0 { 0.0 } else { steps[idx - 1].finish_time_seconds };
                        let end_seconds = step.finish_time_seconds;
                        let time_label = format!("{} -> {}", format_duration(start_seconds), format_duration(end_seconds));
                        let pre_eco = pre_step_eco(&steps, &initial_eco, idx);
                        let step_reasoning = reasoning.get(idx).cloned();

                        rsx! {
                            div { class: "flex flex-col",
                                div {
                                    class: "{row_class}",
                                    role: "button",
                                    tabindex: "0",
                                    onclick: move |_| {
                                        if is_selected {
                                            selected_step.set(None);
                                        } else {
                                            selected_step.set(Some(idx));
                                        }
                                    },
                                    span { class: "text-xs font-mono text-neutral-500 w-6 shrink-0 text-right", "#{idx + 1}" }
                                    span { class: "text-xs font-mono text-sky-300 shrink-0 w-36 text-right", "{time_label}" }
                                    span { class: "flex-1 min-w-0 text-sm text-neutral-200 truncate", "{description}" }
                                    CopyStepButton {
                                        idx,
                                        start_seconds,
                                        end_seconds,
                                        step: step.clone(),
                                        reasoning: step_reasoning.clone(),
                                        pre_eco: pre_eco.clone(),
                                    }
                                }
                                if is_selected {
                                    StepDetails {
                                        step: step.clone(),
                                        reasoning: step_reasoning,
                                        pre_eco,
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
fn CopyStepButton(
    idx: usize,
    start_seconds: f64,
    end_seconds: f64,
    step: StepResult,
    reasoning: Option<StepReasoning>,
    pre_eco: EcoSnapshot,
) -> Element {
    rsx! {
        button {
            class: "ml-2 px-1.5 py-0.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-400 hover:text-white border border-neutral-700 transition-colors shrink-0",
            title: "Copy step details for debug",
            onclick: move |e: MouseEvent| {
                e.stop_propagation();
                let payload = serde_json::json!({
                    "step_number": idx + 1,
                    "start_seconds": start_seconds,
                    "end_seconds": end_seconds,
                    "action": step.action,
                    "economy_before_decision": pre_eco,
                    "reasoning": reasoning,
                });
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                if let Some(window) = web_sys::window() {
                    let _ = window.navigator().clipboard().write_text(&text);
                }
            },
            "📋"
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
        div { class: "mt-2 rounded border border-neutral-700 bg-neutral-950/60 p-3 flex flex-col lg:flex-row gap-4",
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

fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u32;
    let mins = total / 60;
    let secs = total % 60;
    format!("{:02}:{:02}", mins, secs)
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
        Action::Upgrade {
            from,
            to,
            assisted_by,
        } => {
            let assist_text = if assisted_by.is_empty() {
                String::new()
            } else {
                format!(" (assisted by {})", describe_builders(assisted_by))
            };
            format!(
                "{} upgrade to {}{}",
                kind_label(from),
                kind_label(to),
                assist_text
            )
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
