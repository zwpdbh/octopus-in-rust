use dioxus::prelude::*;
use faf_sim::Time;

use crate::components::{
    AppHeader, EcoPanel, QueueItemCreator, QueueItemList, SimulationPanel, UnitSelectorModal,
};
use crate::route::Route;
use crate::state::{load_plan_from_storage, save_plan_to_storage};
use crate::types::{AssignmentTarget, ConstructionItem, UnitSummary};

#[component]
pub fn SimulateBuild() -> Element {
    let units_res = use_context::<Resource<Option<Vec<UnitSummary>>>>();
    let mut plan = use_signal(|| load_plan_from_storage().unwrap_or_default());
    let mut draft_builder = use_signal(|| None::<UnitSummary>);
    let mut draft_builder_count = use_signal(|| 1_u32);
    let mut draft_target = use_signal(|| None::<UnitSummary>);
    let mut draft_target_count = use_signal(|| 1_u32);
    let mut simulation_requested = use_signal(|| false);
    let mut pending_target = use_signal(|| None::<AssignmentTarget>);

    use_effect(move || {
        save_plan_to_storage(&plan.read());
    });

    let assign_unit = move |unit: UnitSummary| {
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
                start_after: Time::from_raw(0.0),
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
                        EcoPanel { plan }
                        div { class: "my-4 border-t border-neutral-700" }
                        QueueItemCreator {
                            draft_builder,
                            draft_builder_count,
                            draft_target,
                            draft_target_count,
                            on_assign_slot: move |target: AssignmentTarget| pending_target.set(Some(target)),
                            on_save: save_draft,
                            on_clear: clear_draft,
                        }
                    }
                    // Right area: created queue (top) + simulation results (bottom)
                    div { class: "flex-1 overflow-hidden flex flex-col",
                        div { class: "flex-1 overflow-hidden flex flex-col p-4 border-b border-neutral-800 bg-neutral-900/30",
                            h3 { class: "text-sm font-semibold text-white mb-3 shrink-0",
                                "Construction Plan"
                            }
                            QueueItemList {
                                plan,
                                on_assign_slot: move |target: AssignmentTarget| pending_target.set(Some(target)),
                            }
                        }
                        div { class: "flex-1 overflow-hidden flex flex-col p-4 bg-neutral-900/30",
                            div { class: "flex items-center justify-center mb-3 shrink-0",
                                button {
                                    class: "px-4 py-1.5 text-sm rounded bg-blue-700 hover:bg-blue-600 text-white transition-colors",
                                    onclick: move |_| {
                                        let current = *simulation_requested.read();
                                        simulation_requested.set(!current);
                                    },
                                    if *simulation_requested.read() {
                                        "Hide Simulation"
                                    } else {
                                        "Run Simulation"
                                    }
                                }
                            }
                            if *simulation_requested.read() {
                                SimulationPanel {
                                    plan: plan.read().clone(),
                                    on_close: move |_| simulation_requested.set(false),
                                }
                            } else {
                                p { class: "text-sm text-neutral-500 text-center mt-4",
                                    "Click \"Run Simulation\" to see build timings."
                                }
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
            }
        },
        Some(None) => rsx! { "Failed to load units" },
        None => rsx! { "Loading..." },
    }
}
