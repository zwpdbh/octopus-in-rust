use dioxus::prelude::*;

use faf_solver::PlanResult;

use crate::components::{QueueItemList, ScheduleFormState, ScheduleModeTab, StepTimeline};
use crate::types::{AssignmentTarget, ConstructionPlan, ScheduleUiState, UnitKind};
use crate::utils::kind_label;

/// Which result tab is active: the computed timeline or the read-only plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultTab {
    Timeline,
    Plan,
}

/// Center column: scheduling result — summary, timeline/plan tabs, actions.
#[component]
pub fn ScheduleResultPanel(
    state: Signal<ScheduleUiState>,
    form: Signal<ScheduleFormState>,
    mut tab: Signal<ResultTab>,
    on_step_click: EventHandler<UnitKind>,
    on_open_map: EventHandler<()>,
    on_send_to_simulate: EventHandler<()>,
) -> Element {
    // The read-only plan view needs a Signal<ConstructionPlan>; mirror the
    // solved plan out of the result state whenever it changes.
    let mut plan_signal = use_signal(ConstructionPlan::default);
    let plan_estimate = use_signal(|| None::<PlanResult>);
    use_effect(move || {
        if let ScheduleUiState::Success(schedule) = &*state.read() {
            plan_signal.set(schedule.plan.clone());
        }
    });

    let current = state.read().clone();
    let active_tab = *tab.read();

    rsx! {
        div { class: "flex-1 min-w-0 flex flex-col border border-neutral-800 rounded bg-neutral-900 p-4 overflow-hidden",
            div { class: "flex items-center gap-2 mb-3 shrink-0",
                button {
                    class: "px-2 py-1 text-xs rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    title: "Show dependency map",
                    onclick: move |_| on_open_map.call(()),
                    "🗺 Map"
                }
                h3 { class: "text-sm font-semibold text-white flex-1 text-right", "Result" }
            }

            match current {
                ScheduleUiState::Idle => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-neutral-500 text-sm",
                        "Configure a target on the left and hit Compute."
                    }
                },
                ScheduleUiState::Computing => rsx! {
                    div { class: "flex-1 flex items-center justify-center text-neutral-400 text-sm",
                        "⚡ Searching for a build order..."
                    }
                },
                ScheduleUiState::Failed(message) => rsx! {
                    div { class: "flex-1 flex flex-col items-center justify-center gap-2",
                        p { class: "text-red-400 text-sm font-semibold", "Scheduling failed" }
                        p { class: "text-neutral-400 text-sm", "{message}" }
                    }
                },
                ScheduleUiState::Success(schedule) => {
                    let step_count = schedule.steps.len();
                    let total = schedule.total_time_seconds;
                    let form_state = form.read();
                    let initial_mass = form_state.initial_mass_production;
                    let final_mass = schedule.final_eco.production_per_second_mass;
                    let final_energy = schedule.final_eco.production_per_second_energy;
                    let is_eco = form_state.mode == ScheduleModeTab::Eco;
                    let target_met = final_mass + form_state.tolerance >= form_state.target_mass_production;
                    let headline = match form_state.mode {
                        ScheduleModeTab::Eco => {
                            format!("Reached eco target in {total:.1}s")
                        }
                        ScheduleModeTab::Unit => {
                            let name = form_state
                                .unit_target
                                .as_ref()
                                .map(kind_label)
                                .unwrap_or_else(|| "unit".to_string());
                            format!("Built {name} in {total:.1}s")
                        }
                    };
                    rsx! {
                        div { class: "flex flex-col gap-3 flex-1 min-h-0",
                            // Summary banner.
                            div { class: "shrink-0 rounded border border-emerald-800 bg-emerald-950/40 px-3 py-2",
                                p { class: "text-sm font-semibold text-emerald-300", "✓ {headline} ({step_count} steps)" }
                                p { class: "text-xs text-neutral-300 mt-1",
                                    "Mass income {initial_mass:.0}/s → {final_mass:.0}/s"
                                    if is_eco {
                                        {
                                            let mark = if target_met { "✓" } else { "✗" };
                                            rsx! { " (target {form_state.target_mass_production:.0}/s {mark})" }
                                        }
                                    }
                                }
                                p { class: "text-xs text-neutral-300", "Energy income → {final_energy:.0}/s" }
                            }

                            // Tabs.
                            div { class: "flex items-center gap-1 p-1 rounded bg-neutral-950 border border-neutral-800 shrink-0 self-start",
                                TabButton {
                                    label: "Timeline",
                                    active: active_tab == ResultTab::Timeline,
                                    onclick: move |_| tab.set(ResultTab::Timeline),
                                }
                                TabButton {
                                    label: "Plan",
                                    active: active_tab == ResultTab::Plan,
                                    onclick: move |_| tab.set(ResultTab::Plan),
                                }
                            }

                            // Tab content.
                            match active_tab {
                                ResultTab::Timeline => rsx! {
                                    StepTimeline { steps: schedule.steps.clone(), on_click: on_step_click }
                                },
                                ResultTab::Plan => rsx! {
                                    QueueItemList {
                                        plan: plan_signal,
                                        plan_estimate,
                                        disabled: true,
                                        on_assign_slot: move |_: AssignmentTarget| {},
                                    }
                                },
                            }

                            // Actions.
                            div { class: "flex items-center gap-2 shrink-0",
                                button {
                                    class: "px-3 py-1.5 text-xs font-semibold rounded bg-emerald-600 hover:bg-emerald-500 text-white transition-colors",
                                    onclick: move |_| on_send_to_simulate.call(()),
                                    "⚡ Send to Simulate"
                                }
                                CopyPlanButton { plan: schedule.plan.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TabButton(label: &'static str, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if active {
                "px-3 py-1 text-xs font-semibold rounded bg-blue-700 text-white transition-colors"
            } else {
                "px-3 py-1 text-xs font-semibold rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
            },
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

#[component]
fn CopyPlanButton(plan: ConstructionPlan) -> Element {
    let mut copied = use_signal(|| false);
    rsx! {
        button {
            class: "px-3 py-1.5 text-xs font-semibold rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors",
            onclick: move |_| {
                let text = serde_json::to_string_pretty(&plan)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                if let Some(window) = web_sys::window() {
                    let _ = window.navigator().clipboard().write_text(&text);
                }
                copied.set(true);
            },
            if *copied.read() { "Copied!" } else { "Copy JSON" }
        }
    }
}
