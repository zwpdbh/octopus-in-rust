use dioxus::prelude::*;
use faf_quantities::Time;
use faf_sim_shared::BuildQueue;
use faf_solver::{plan_completion_with_tasks, PlanResult};
use gloo_net::http::Request;

use crate::components::{
    AppHeader, EcoPanel, GraphPopup, QueueItemCreator, QueueItemList, SimulationPanel,
    UnitSelectorModal,
};
use crate::route::Route;
use crate::state::{load_plan_from_storage, save_plan_to_storage};
use crate::types::{
    AssignmentTarget, BlueprintGraphResponse, ConstructionItem, ConstructionPlan,
    SimulationUiState, UnitSummary,
};

#[component]
pub fn SimulateBuild() -> Element {
    let units_res = use_context::<Resource<Option<Vec<UnitSummary>>>>();
    let mut plan = use_signal(|| load_plan_from_storage().unwrap_or_default());
    let mut draft_builder = use_signal(|| None::<UnitSummary>);
    let mut draft_builder_count = use_signal(|| 1_u32);
    let mut draft_target = use_signal(|| None::<UnitSummary>);
    let mut draft_target_count = use_signal(|| 1_u32);
    let simulation_state = use_signal(|| SimulationUiState::NotStartYet);
    let mut show_json_editor = use_signal(|| false);
    let mut pending_target = use_signal(|| None::<AssignmentTarget>);
    let mut plan_estimate = use_signal(|| None::<PlanResult>);
    let mut map_focus = use_signal(|| None::<UnitSummary>);
    let mut show_map = use_signal(|| false);

    // Concrete dependency graph for the map popup.
    let graph = use_resource(|| async move {
        Request::get("/api/blueprint-graph")
            .send()
            .await
            .ok()?
            .json::<BlueprintGraphResponse>()
            .await
            .ok()
    });

    use_effect(move || {
        save_plan_to_storage(&plan.read());
    });

    // Clear stale solver results whenever the user edits the plan.
    use_effect(move || {
        let _ = plan.read();
        plan_estimate.set(None);
    });

    let assign_unit = move |unit: UnitSummary| {
        map_focus.set(Some(unit.clone()));
        if let Some(target) = *pending_target.read() {
            match target {
                AssignmentTarget::ExistingBuilder { item_id } => {
                    plan.with_mut(|p| {
                        if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                            i.builders = vec![unit];
                        }
                    });
                }
                AssignmentTarget::ExistingTarget { item_id } => {
                    plan.with_mut(|p| {
                        if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                            i.targets = vec![unit];
                        }
                    });
                }
                AssignmentTarget::NewBuilder => draft_builder.set(Some(unit)),
                AssignmentTarget::NewTarget => draft_target.set(Some(unit)),
            }
        }
        pending_target.set(None);
    };

    let save_draft = move |_| {
        let builder = draft_builder.read().clone();
        let target = draft_target.read().clone();
        if let (Some(builder), Some(target)) = (builder, target) {
            let builder_count = (*draft_builder_count.read()).max(1);
            let target_count = (*draft_target_count.read()).max(1);
            let next_id = plan.read().items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            let item = ConstructionItem {
                id: next_id,
                builders: vec![builder; builder_count as usize],
                targets: vec![target; target_count as usize],
                start_after: Time::from_raw(1.0),
            };
            if item.is_valid() {
                plan.write().items.push(item);
                draft_builder.set(None);
                draft_target.set(None);
                draft_builder_count.set(1);
                draft_target_count.set(1);
            }
        }
    };

    let clear_draft = move |_| {
        draft_builder.set(None);
        draft_target.set(None);
        draft_builder_count.set(1);
        draft_target_count.set(1);
    };

    let units_data = units_res.read().clone();
    match units_data {
        Some(Some(units)) => rsx! {
            div { class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans overflow-hidden select-none",
                AppHeader { active: Route::SimulateBuild {} }
                div { class: "flex flex-1 overflow-hidden",
                    // Left sidebar: Eco Settings + new item creator
                    div { class: "w-80 shrink-0 overflow-auto p-4 border-r border-neutral-800 bg-neutral-900/30",
                        div {
                            class: if plan_locked(simulation_state) { "pointer-events-none opacity-60" } else { "" },
                            EcoPanel {
                                plan,
                                disabled: plan_locked(simulation_state),
                            }
                            div { class: "my-4 border-t border-neutral-700" }
                            QueueItemCreator {
                                draft_builder,
                                draft_builder_count,
                                draft_target,
                                draft_target_count,
                                disabled: plan_locked(simulation_state),
                                on_assign_slot: move |target: AssignmentTarget| pending_target.set(Some(target)),
                                on_save: save_draft,
                                on_clear: clear_draft,
                            }
                        }
                    }
                    // Right area: created queue (top) + simulation results (bottom)
                    div { class: "flex-1 overflow-hidden flex flex-col",
                        div { class: "flex-1 overflow-hidden flex flex-col p-4 border-b border-neutral-800 bg-neutral-900/30",
                            div { class: "flex items-center gap-2 mb-3 shrink-0",
                                h3 { class: "text-sm font-semibold text-white", "Construction Plan" }
                                button {
                                    class: "px-2 py-1 text-xs rounded bg-emerald-600 hover:bg-emerald-500 text-white transition-colors font-mono shadow-sm",
                                    title: "Estimate plan impact",
                                    onclick: move |_| {
                                        let snapshot = plan.read().eco.to_snapshot();
                                        let queue = plan.read().to_build_queue();
                                        plan_estimate.set(Some(plan_completion_with_tasks(
                                            &snapshot,
                                            &queue.tasks,
                                            6000.0,
                                        )));
                                    },
                                    "⚡"
                                }
                                button {
                                    class: "px-2 py-1 text-xs rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors font-mono shadow-sm",
                                    title: if *show_json_editor.read() { "Show cards" } else { "Show JSON" },
                                    onclick: move |_| {
                                        let current = *show_json_editor.read();
                                        show_json_editor.set(!current);
                                    },
                                    if *show_json_editor.read() { "☰" } else { "{{ }}" }
                                }
                                button {
                                    class: "px-2 py-1 text-xs rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 border border-neutral-700 transition-colors shadow-sm",
                                    title: "Show dependency map",
                                    onclick: move |_| show_map.set(true),
                                    "🗺"
                                }
                                {
                                    let locked = plan_locked(simulation_state);
                                    rsx! {
                                        button {
                                            class: if locked {
                                                "px-2 py-1 text-xs rounded bg-red-900/50 text-red-300/50 cursor-not-allowed shadow-sm"
                                            } else {
                                                "px-2 py-1 text-xs rounded bg-red-700 hover:bg-red-600 text-white transition-colors shadow-sm"
                                            },
                                            title: if locked { "Cannot clear while simulating" } else { "Clear construction plan" },
                                            disabled: locked,
                                            onclick: move |_| {
                                                if !locked {
                                                    plan.write().items.clear();
                                                }
                                            },
                                            "Clear"
                                        }
                                    }
                                }
                            }
                            if *show_json_editor.read() {
                                JsonPlanEditor { plan, units: units.clone() }
                            } else {
                                QueueItemList {
                                    plan,
                                    plan_estimate,
                                    disabled: plan_locked(simulation_state),
                                    on_assign_slot: move |target: AssignmentTarget| pending_target.set(Some(target)),
                                }
                            }
                        }
                        div { class: "flex-1 overflow-hidden flex flex-col p-4 bg-neutral-900/30",
                            SimulationPanel {
                                plan,
                                state: simulation_state,
                            }
                        }
                    }
                }
                UnitSelectorModal {
                    open: pending_target.read().is_some(),
                    units,
                    target: (*pending_target.read()).unwrap_or(AssignmentTarget::NewTarget),
                    on_select: assign_unit,
                    on_close: move |_| pending_target.set(None),
                }
                if let Some(data) = graph.read().clone().flatten() {
                    GraphPopup {
                        open: *show_map.read(),
                        data,
                        focus: map_focus.read().clone(),
                        on_node_click: move |summary: UnitSummary| map_focus.set(Some(summary)),
                        on_close: move |_| show_map.set(false),
                    }
                }
            }
        },
        Some(None) => rsx! { "Failed to load units" },
        None => rsx! { "Loading..." },
    }
}

#[component]
fn JsonPlanEditor(mut plan: Signal<ConstructionPlan>, units: Vec<UnitSummary>) -> Element {
    let units = use_signal(|| units);
    let mut json_text = use_signal(|| serialize_build_queue(&plan.read()));
    let mut error = use_signal(|| String::new());
    let mut copied = use_signal(|| false);

    // Reset the editor text whenever the plan changes from outside the editor
    // (e.g. adding an item via the cards view).
    use_effect(move || {
        json_text.set(serialize_build_queue(&plan.read()));
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
                    match serde_json::from_str::<BuildQueue>(&text) {
                        Ok(queue) => {
                            plan.set(ConstructionPlan::from_build_queue_with_units(queue, &units.read()));
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

fn serialize_build_queue(plan: &ConstructionPlan) -> String {
    serde_json::to_string_pretty(&plan.to_build_queue())
        .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn copy_to_clipboard(text: &str) {
    if let Some(window) = web_sys::window() {
        let clipboard = window.navigator().clipboard();
        let _ = clipboard.write_text(text);
    }
}

fn plan_locked(state: Signal<SimulationUiState>) -> bool {
    matches!(
        *state.read(),
        SimulationUiState::Running | SimulationUiState::Paused
    )
}
