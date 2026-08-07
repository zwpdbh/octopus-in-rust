use dioxus::prelude::*;
use faf_blueprints::{ConstructionAction, ConstructionPlan, UnitBlueprint};
use gloo_net::http::Request;

use super::{PortraitButton, UnitSummary};
use crate::components::UnitSelectorModal;

/// A single entry in the visual queue builder.
///
/// The target is optional while the action is being constructed; only actions
/// with a target are serialized into the underlying `ConstructionPlan`.
#[derive(Clone)]
struct QueueAction {
    builders: Vec<UnitBlueprint>,
    target: Option<UnitBlueprint>,
}

#[derive(Clone, PartialEq)]
enum SelectorMode {
    Target { index: usize },
    Builder { index: usize },
}

/// Visual editor for the construction queue.
///
/// Reads the initial queue from `plan_json`, lets the user add/remove actions
/// and pick builders/targets from a searchable unit modal, and writes the
/// updated queue back to `plan_json`.
#[component]
pub fn QueueBuilder(plan_json: Signal<String>) -> Element {
    let mut queue = use_signal(Vec::<QueueAction>::new);
    let units = use_resource(move || async move {
        Request::get("http://localhost:3000/api/units")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json::<Vec<UnitSummary>>()
            .await
            .map_err(|e| e.to_string())
    });
    let mut selector_open = use_signal(|| false);
    let mut selector_mode = use_signal(|| None::<SelectorMode>);

    let unit_list = match units.read().as_ref() {
        Some(Ok(list)) => list.clone(),
        Some(Err(err)) => {
            return rsx! {
                div { class: "text-xs text-red-400", "Failed to load units: {err}" }
            };
        }
        None => {
            return rsx! {
                div { class: "text-xs text-neutral-400", "Loading units..." }
            };
        }
    };

    // Keep the visual queue in sync with external changes to the JSON plan.
    use_effect(move || {
        if let Ok(plan) = serde_json::from_str::<ConstructionPlan>(&plan_json()) {
            queue.set(
                plan.building_queue()
                    .iter()
                    .map(|action| QueueAction {
                        builders: action.builders().to_vec(),
                        target: Some(action.target().clone()),
                    })
                    .collect(),
            );
        }
    });

    let mut open_target_selector = move |index: usize| {
        selector_mode.set(Some(SelectorMode::Target { index }));
        selector_open.set(true);
    };

    let mut open_builder_selector = move |index: usize| {
        selector_mode.set(Some(SelectorMode::Builder { index }));
        selector_open.set(true);
    };

    let on_select_unit = move |unit: UnitSummary| {
        let blueprint = unit.to_blueprint();
        queue.with_mut(|q| match selector_mode() {
            Some(SelectorMode::Target { index }) => {
                if let Some(action) = q.get_mut(index) {
                    action.target = Some(blueprint);
                }
            }
            Some(SelectorMode::Builder { index }) => {
                if let Some(action) = q.get_mut(index) {
                    action.builders.push(blueprint);
                }
            }
            None => {}
        });
        selector_open.set(false);
        sync_plan_json(&queue, plan_json);
    };

    let unit_list_for_add = unit_list.clone();
    let add_action = move |_| {
        if let Some(first) = unit_list_for_add.first() {
            queue.with_mut(|q| {
                q.push(QueueAction {
                    builders: Vec::new(),
                    target: Some(first.to_blueprint()),
                })
            });
            sync_plan_json(&queue, plan_json);
        }
    };

    let mut remove_action = move |index: usize| {
        queue.with_mut(|q| {
            q.remove(index);
        });
        sync_plan_json(&queue, plan_json);
    };

    let mut remove_builder = move |(action_index, builder_index): (usize, usize)| {
        queue.with_mut(|q| {
            if let Some(action) = q.get_mut(action_index) {
                action.builders.remove(builder_index);
            }
        });
        sync_plan_json(&queue, plan_json);
    };

    let mut remove_target = move |index: usize| {
        queue.with_mut(|q| {
            q.remove(index);
        });
        sync_plan_json(&queue, plan_json);
    };

    rsx! {
        div { class: "flex flex-col gap-3",
            for (index , action) in queue.read().iter().enumerate() {
                div { key: "{index}", class: "border border-neutral-700 rounded-lg bg-neutral-800/50 p-3",
                    div { class: "flex items-start justify-between gap-2 mb-2",
                        span { class: "text-xs font-medium text-neutral-400", "Action #{index + 1}" }
                        button {
                            class: "text-neutral-500 hover:text-red-400 text-xs",
                            onclick: move |_| remove_action(index),
                            "Remove"
                        }
                    }

                    div { class: "mb-3",
                        div { class: "text-[10px] uppercase tracking-wider text-neutral-500 mb-1", "Target" }
                        if let Some(target) = action.target.as_ref() {
                            div { class: "flex items-center gap-2",
                                PortraitButton {
                                    unit_id: target.unit_id().to_string(),
                                    label: target.unit_description().to_string(),
                                    selected: true,
                                    on_click: move |_| open_target_selector(index),
                                }
                                button {
                                    class: "text-neutral-500 hover:text-red-400 text-xs",
                                    onclick: move |_| remove_target(index),
                                    "✕"
                                }
                            }
                        } else {
                            button {
                                class: "px-3 py-2 rounded border border-dashed border-neutral-600 text-xs text-neutral-300 hover:border-blue-500 hover:text-blue-400",
                                onclick: move |_| open_target_selector(index),
                                "Select target"
                            }
                        }
                    }

                    div {
                        div { class: "text-[10px] uppercase tracking-wider text-neutral-500 mb-1", "Builders" }
                        div { class: "flex flex-wrap items-center gap-2",
                            for (builder_index , builder) in action.builders.iter().enumerate() {
                                div { key: "{builder_index}", class: "flex items-center gap-1",
                                    PortraitButton {
                                        unit_id: builder.unit_id().to_string(),
                                        label: builder.unit_description().to_string(),
                                        selected: false,
                                        on_click: move |_| {},
                                    }
                                    button {
                                        class: "text-neutral-500 hover:text-red-400 text-xs",
                                        onclick: move |_| remove_builder((index, builder_index)),
                                        "✕"
                                    }
                                }
                            }
                            button {
                                class: "px-3 py-2 rounded border border-dashed border-neutral-600 text-xs text-neutral-300 hover:border-blue-500 hover:text-blue-400",
                                onclick: move |_| open_builder_selector(index),
                                "Add builder"
                            }
                        }
                    }
                }
            }

            button {
                class: "px-3 py-2 rounded border border-dashed border-neutral-600 text-xs text-neutral-300 hover:border-emerald-500 hover:text-emerald-400",
                onclick: add_action,
                "+ Add Action"
            }

            UnitSelectorModal {
                units: unit_list,
                open: selector_open,
                on_select: on_select_unit,
            }
        }
    }
}

/// Serialize the current visual queue back into `plan_json`, preserving the
/// existing `player_eco` section.
fn sync_plan_json(queue: &Signal<Vec<QueueAction>>, mut plan_json: Signal<String>) {
    if let Ok(plan) = serde_json::from_str::<ConstructionPlan>(&plan_json()) {
        let actions: Vec<ConstructionAction> = queue
            .read()
            .iter()
            .filter(|action| action.target.is_some())
            .map(|action| {
                ConstructionAction::new(action.builders.clone(), action.target.clone().unwrap())
            })
            .collect();

        let new_plan = ConstructionPlan::new(plan.player_eco().clone(), actions);
        if let Ok(json) = serde_json::to_string_pretty(&new_plan) {
            plan_json.set(json);
        }
    }
}
