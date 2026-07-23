use dioxus::prelude::*;

use crate::components::{ScheduleFormState, ScheduleModeTab, StepTimeline};
use crate::types::{ConstructionPlan, Schedule, ScheduleUiState, StepReasoning};
use crate::utils::kind_label;

/// Center column: scheduling result.
///
/// The panel is organized into three UX parts for finished results:
/// 1. Result status header — success/failure summary.
/// 2. Result body — step timeline on success, failure reason on failure.
/// 3. Result actions footer — send to simulate and copy JSON (success only).
#[component]
pub fn ScheduleResultPanel(
    state: Signal<ScheduleUiState>,
    form: Signal<ScheduleFormState>,
    selected_step: Signal<Option<usize>>,
    reasoning: Vec<StepReasoning>,
    on_open_map: EventHandler<()>,
    on_send_to_simulate: EventHandler<()>,
) -> Element {
    let current = state.read().clone();

    rsx! {
        div { class: "flex-1 min-w-0 flex flex-col border border-neutral-800 rounded bg-neutral-900 p-4 overflow-hidden",
            ResultHeader { on_open_map }

            match current {
                ScheduleUiState::Idle => rsx! { ResultIdle {} },
                ScheduleUiState::Streaming { steps, .. } => rsx! {
                    ResultStreaming {
                        steps,
                        reasoning,
                        form,
                        selected_step,
                    }
                },
                ScheduleUiState::Failed(message) => rsx! {
                    ResultFailed { message, form }
                },
                ScheduleUiState::Success(schedule, reasoning) => rsx! {
                    ResultSuccess {
                        schedule,
                        reasoning,
                        form,
                        selected_step,
                        on_send_to_simulate,
                    }
                },
            }
        }
    }
}

/// Persistent result-card header: map action + title.
#[component]
fn ResultHeader(on_open_map: EventHandler<()>) -> Element {
    rsx! {
        div { class: "flex items-center gap-2 mb-3 shrink-0",
            button {
                class: "px-2 py-1 text-xs rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                title: "Show dependency map",
                onclick: move |_| on_open_map.call(()),
                "🗺 Map"
            }
            h3 { class: "text-sm font-semibold text-white flex-1 text-right", "Result" }
        }
    }
}

/// Empty state shown before the user starts a schedule.
#[component]
fn ResultIdle() -> Element {
    rsx! {
        div { class: "flex-1 flex items-center justify-center text-neutral-500 text-sm",
            "Configure a target on the left and hit Compute."
        }
    }
}

/// State shown while the scheduler is streaming steps over the WebSocket.
#[component]
fn ResultStreaming(
    steps: Vec<crate::types::StepResult>,
    reasoning: Vec<StepReasoning>,
    form: Signal<ScheduleFormState>,
    selected_step: Signal<Option<usize>>,
) -> Element {
    let step_count = steps.len();

    rsx! {
        div { class: "flex flex-col gap-3 flex-1 min-h-0",
            // Streaming progress banner.
            div { class: "shrink-0 rounded border border-blue-800 bg-blue-950/40 px-3 py-2",
                p { class: "text-sm font-semibold text-blue-300", "⚡ Scheduling in progress..." }
                p { class: "text-xs text-neutral-300 mt-1", "{step_count} step(s) committed so far" }
            }

            // Partial timeline.
            StepTimeline {
                steps,
                reasoning,
                initial_eco: form.read().initial_snapshot(),
                selected_step,
            }
        }
    }
}

/// Failure result: status header + failure reason body.
#[component]
fn ResultFailed(message: String, form: Signal<ScheduleFormState>) -> Element {
    rsx! {
        div { class: "flex flex-col gap-3 flex-1 min-h-0",
            ResultStatusHeader {
                status: ResultStatus::Failed,
                form,
            }
            ResultFailureBody { message }
        }
    }
}

/// Success result: status header + timeline + actions footer.
#[component]
fn ResultSuccess(
    schedule: Schedule,
    reasoning: Vec<StepReasoning>,
    form: Signal<ScheduleFormState>,
    selected_step: Signal<Option<usize>>,
    on_send_to_simulate: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-3 flex-1 min-h-0",
            ResultStatusHeader {
                status: ResultStatus::Success(schedule.clone()),
                form,
            }

            StepTimeline {
                steps: schedule.steps.clone(),
                reasoning,
                initial_eco: form.read().initial_snapshot(),
                selected_step,
            }

            ResultActionsFooter {
                plan: schedule.plan.clone(),
                on_send_to_simulate,
            }
        }
    }
}

/// Status of a finished scheduling run, used by the unified status header.
#[derive(Clone, PartialEq)]
enum ResultStatus {
    Success(Schedule),
    Failed,
}

/// Success or failure summary banner at the top of a finished result.
#[component]
fn ResultStatusHeader(status: ResultStatus, form: Signal<ScheduleFormState>) -> Element {
    match status {
        ResultStatus::Success(schedule) => rsx! { ResultSuccessBanner { schedule, form } },
        ResultStatus::Failed => rsx! { ResultFailureBanner {} },
    }
}

/// Green banner summarizing a successful schedule.
#[component]
fn ResultSuccessBanner(schedule: Schedule, form: Signal<ScheduleFormState>) -> Element {
    let form_state = form.read();
    let step_count = schedule.steps.len();
    let total = schedule.total_time_seconds;
    let initial_mass = form_state.initial_mass_production;
    let final_mass = schedule.final_eco.production_per_second_mass.value();
    let final_energy = schedule.final_eco.production_per_second_energy.value();
    let is_eco = form_state.mode == ScheduleModeTab::Eco;
    let target_met = final_mass + form_state.tolerance >= form_state.target_mass_production;
    let headline = match form_state.mode {
        ScheduleModeTab::Eco => format!("Reached eco target in {total:.1}s"),
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
    }
}

/// Red banner summarizing a failed schedule.
#[component]
fn ResultFailureBanner() -> Element {
    rsx! {
        div { class: "shrink-0 rounded border border-red-800 bg-red-950/40 px-3 py-2",
            p { class: "text-sm font-semibold text-red-300", "✗ Scheduling failed" }
        }
    }
}

/// Detailed failure reason shown in the result body.
#[component]
fn ResultFailureBody(message: String) -> Element {
    rsx! {
        div { class: "flex-1 flex flex-col items-center justify-center gap-2",
            p { class: "text-red-400 text-sm font-semibold", "Scheduling failed" }
            p { class: "text-neutral-400 text-sm", "{message}" }
        }
    }
}

/// Bottom action bar for a successful result.
#[component]
fn ResultActionsFooter(plan: ConstructionPlan, on_send_to_simulate: EventHandler<()>) -> Element {
    rsx! {
        div { class: "flex items-center gap-2 shrink-0",
            button {
                class: "px-3 py-1.5 text-xs font-semibold rounded bg-emerald-600 hover:bg-emerald-500 text-white transition-colors",
                onclick: move |_| on_send_to_simulate.call(()),
                "⚡ Send to Simulate"
            }
            CopyPlanButton { plan }
        }
    }
}

/// Copy the completed construction plan as pretty-printed JSON.
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
