use dioxus::prelude::*;
use faf_sim::GameEcoParameters;

use crate::components::{
    inventory_after_steps, AxisSide, ChartMetric, CurrentUnits, DualAxisSeries, DualAxisUplotChart,
    EcoSnapshotView, RGBColor, ScheduleFormState, ScheduleModeTab, StepTimeline,
};
use crate::types::{ConstructionPlan, Schedule, ScheduleUiState, StepReasoning};
use crate::utils::kind_label;

/// Single data point for the dual-axis net-income chart.
#[derive(Clone, PartialEq)]
struct IncomePoint {
    time: f64,
    mass: f64,
    energy: f64,
}

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
                ScheduleUiState::Idle => rsx! {
                    ResultIdle {}
                },
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
                initial_eco: form.read().init_eco(),
                initial_inventory: form.read().initial_inventory.clone(),
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
            ResultStatusHeader { status: ResultStatus::Failed, form }
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
            ResultStatusHeader { status: ResultStatus::Success(schedule.clone()), form }

            StepTimeline {
                steps: schedule.steps.clone(),
                reasoning,
                initial_eco: form.read().init_eco(),
                initial_inventory: form.read().initial_inventory.clone(),
                selected_step,
            }

            ResultActionsFooter { plan: schedule.plan.clone(), on_send_to_simulate }
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
        ResultStatus::Success(schedule) => rsx! {
            ResultSuccessBanner { schedule, form }
        },
        ResultStatus::Failed => rsx! {
            ResultFailureBanner {}
        },
    }
}

/// Economy state after the schedule's final committed step.
///
/// This is the counterpart to [`step_timeline::EconomyBeforeDecision`]; it shows
/// the economy once every step has been applied rather than the initial state.
#[component]
fn EconomyAfterFinalStep(
    eco: GameEcoParameters,
    #[props(default = "flex flex-col gap-1 min-w-0")] class: &'static str,
) -> Element {
    rsx! {
        div { class: "{class}",
            h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                "Economy after final step"
            }
            div { class: "max-w-2xl",
                EcoSnapshotView { snapshot: eco, compact: true }
            }
        }
    }
}

/// Green card summarizing a successful schedule.
///
/// The card composites the final economy snapshot with the final unit
/// inventory, mirroring the per-step detail panels in the timeline.
#[component]
fn ResultSuccessBanner(schedule: Schedule, form: Signal<ScheduleFormState>) -> Element {
    let form_state = form.read();
    let step_count = schedule.steps.len();
    let total = schedule.total_time_seconds;
    let initial_mass = form_state.initial_mass_production;
    let initial_energy = form_state.initial_energy_production;
    let final_mass = schedule.final_eco.production_per_second_mass.value();
    let final_energy = schedule.final_eco.production_per_second_energy.value();
    let is_eco = form_state.mode == ScheduleModeTab::Eco;
    let target_met = final_mass + form_state.tolerance >= form_state.target_mass_production;
    let headline = match form_state.mode {
        ScheduleModeTab::Eco => "Reached eco target".to_string(),
        ScheduleModeTab::Unit => {
            let name = form_state
                .unit_target
                .as_ref()
                .map(kind_label)
                .unwrap_or_else(|| "unit".to_string());
            format!("Built {name}")
        }
    };
    let target_mark = if target_met { "✓" } else { "✗" };
    let target_class = if target_met {
        "text-emerald-400"
    } else {
        "text-red-400"
    };
    let final_units = inventory_after_steps(&form_state.initial_inventory, &schedule.steps);

    let income_data: Vec<IncomePoint> = std::iter::once(IncomePoint {
        time: 0.0,
        mass: form_state.initial_mass_production,
        energy: form_state.initial_energy_production,
    })
    .chain(schedule.steps.iter().map(|s| IncomePoint {
        time: s.finish_time_seconds,
        mass: s.economy.production_per_second_mass.value(),
        energy: s.economy.production_per_second_energy.value()
            - s.economy.maintenance_consumption_per_second_energy.value(),
    }))
    .collect();
    let income_signal = use_signal(|| income_data);

    rsx! {
        div { class: "shrink-0 rounded border border-emerald-800 bg-emerald-950/40 p-3 flex flex-col gap-3",
            // Headline and meta.
            div { class: "flex items-center justify-between gap-2",
                p { class: "text-sm font-semibold text-emerald-300", "✓ {headline}" }
                span { class: "text-xs font-mono text-neutral-400", "{total:.1}s · {step_count} steps" }
            }

            // Compact transition summary as readable stat blocks.
            div { class: "flex flex-wrap items-center gap-x-4 gap-y-1 text-xs",
                div { class: "flex items-center gap-1.5",
                    span { class: "text-[10px] font-semibold text-neutral-500 uppercase tracking-wide",
                        "Mass"
                    }
                    span { class: "font-mono text-neutral-300", "{initial_mass:.0}/s" }
                    span { class: "text-neutral-500", "→" }
                    span { class: "font-mono text-emerald-300", "{final_mass:.0}/s" }
                    if is_eco {
                        span { class: "text-[10px] {target_class}",
                            "(target {form_state.target_mass_production:.0}/s {target_mark})"
                        }
                    }
                }
                div { class: "flex items-center gap-1.5",
                    span { class: "text-[10px] font-semibold text-neutral-500 uppercase tracking-wide",
                        "Energy"
                    }
                    span { class: "font-mono text-neutral-300", "{initial_energy:.0}/s" }
                    span { class: "text-neutral-500", "→" }
                    span { class: "font-mono text-amber-300", "{final_energy:.0}/s" }
                }
            }

            // 2-column dashboard layout:
            //   left: economy and units stacked
            //   right: one big dual-axis chart for mass + energy net income
            div { class: "grid grid-cols-1 lg:grid-cols-[minmax(300px,1fr)_2fr] gap-3 items-stretch",
                div { class: "flex flex-col gap-3",
                    EconomyAfterFinalStep { eco: schedule.final_eco }
                    CurrentUnits {
                        units: final_units,
                        class: "flex flex-col gap-1 flex-1 min-w-0",
                    }
                }
                div { class: "flex flex-col gap-1 min-w-0 h-full",
                    h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                        "Net income over time"
                    }
                    DualAxisUplotChart {
                        data: income_signal,
                        x_extractor: ChartMetric::new(|p: &IncomePoint| p.time),
                        series: vec![
                            DualAxisSeries::new(
                                "Mass",
                                RGBColor(52, 211, 153),
                                AxisSide::Left,
                                ChartMetric::new(|p: &IncomePoint| p.mass),
                            ),
                            DualAxisSeries::new(
                                "Energy",
                                RGBColor(251, 191, 36),
                                AxisSide::Right,
                                ChartMetric::new(|p: &IncomePoint| p.energy),
                            ),
                        ],
                        left_axis_label: "Mass net income (/s)",
                        right_axis_label: "Energy net income (/s)",
                    }
                }
            }
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
            if *copied.read() {
                "Copied!"
            } else {
                "Copy JSON"
            }
        }
    }
}
