use dioxus::prelude::*;
use faf_quantities::Time;
use faf_solver::PlanResult;

use crate::components::UnitBlock;
use crate::types::{AssignmentTarget, ConstructionItem, ConstructionPlan};

const MAX_SOLVER_TIME: f64 = 6_000.0;

#[component]
pub fn ConstructionItemCard(
    item: ConstructionItem,
    plan: Signal<ConstructionPlan>,
    plan_estimate: Signal<Option<PlanResult>>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let item_id = item.id;

    let remove = move |_| {
        if !disabled {
            plan.with_mut(|p| p.items.retain(|i| i.id != item_id));
        }
    };

    let mut adjust_vec = move |field: &'static str, new_len: u32| {
        if disabled {
            return;
        }
        plan.with_mut(|p| {
            if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                match field {
                    "builders" => {
                        let template = i.builders.first().cloned();
                        resize_with_template(&mut i.builders, new_len as usize, template);
                    }
                    "targets" => {
                        let template = i.targets.first().cloned();
                        resize_with_template(&mut i.targets, new_len as usize, template);
                    }
                    _ => {}
                }
            }
        });
    };

    let mut update_start_after = move |value: f64| {
        if disabled {
            return;
        }
        plan.with_mut(|p| {
            if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                i.start_after = Time::from_raw(value.max(0.0));
            }
        });
    };

    // Pull this card's slice out of the plan-level solver result.
    // let estimate = use_memo(move || {
    //     let plan = plan.read();
    //     let index = plan.items.iter().position(|i| i.id == item_id)?;
    //     let initial_eco = plan.eco.to_snapshot();
    //     let result = plan_estimate.read().as_ref()?.clone();

    //     let current = *result.tasks.get(index)?;
    //     let previous = if index == 0 {
    //         CompletionResult {
    //             time_seconds: 0.0,
    //             eco: initial_eco,
    //         }
    //     } else {
    //         *result.tasks.get(index - 1)?
    //     };
    //     Some((current, previous))
    // });

    // let (finish_time, duration, delta) = estimate.read().as_ref().map_or(
    //     (None, None, EcoDelta::default()),
    //     |(current, previous)| {
    //         let finish = current.time_seconds;
    //         let duration = finish - previous.time_seconds;
    //         let delta = EcoDelta {
    //             mass_prod: current.eco.production_per_second_mass.value()
    //                 - previous.eco.production_per_second_mass.value(),
    //             energy_prod: current.eco.production_per_second_energy.value()
    //                 - previous.eco.production_per_second_energy.value(),
    //             mass_storage: current.eco.mass_storage.value() - previous.eco.mass_storage.value(),
    //             energy_storage: current.eco.energy_storage.value()
    //                 - previous.eco.energy_storage.value(),
    //             maintenance: current
    //                 .eco
    //                 .maintenance_consumption_per_second_energy
    //                 .value()
    //                 - previous
    //                     .eco
    //                     .maintenance_consumption_per_second_energy
    //                     .value(),
    //         };
    //         (Some(finish), Some(duration), delta)
    //     },
    // );
    let finish_time = Some(0.0);
    let finish_text = finish_time
        .map(|t| format!("{:.0}s", t))
        .unwrap_or_else(|| "-".to_string());
    let duration_text = Some(100)
        .map(|t| format!("{:.0}s", t))
        .unwrap_or_else(|| "-".to_string());
    let stalled = finish_time.is_some_and(|t| (t - MAX_SOLVER_TIME).abs() < 1e-6);
    let has_estimate = plan_estimate.read().is_some();

    rsx! {
        div { class: "w-full p-3 rounded-lg bg-neutral-800/50 border border-neutral-700 text-sm",
            div { class: "flex items-center justify-between mb-2",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500",
                    "Queue Item"
                }
                button {
                    class: if disabled { "px-2 py-0.5 rounded bg-red-900/20 text-red-300/50 text-xs cursor-not-allowed" } else { "px-2 py-0.5 rounded bg-red-900/40 hover:bg-red-900/60 text-red-300 text-xs transition-colors" },
                    disabled,
                    onclick: remove,
                    "x"
                }
            }
            div { class: "flex flex-col gap-2",
                UnitBlock {
                    label: "Builder",
                    unit: item.builders.first().cloned(),
                    count: item.builders.len() as u32,
                    hint: "Requires build power",
                    disabled,
                    on_click: move |_| {
                        on_assign_slot
                            .call(AssignmentTarget::ExistingBuilder {
                                item_id,
                            })
                    },
                    on_count: move |v: u32| adjust_vec("builders", v),
                }
                UnitBlock {
                    label: "Target",
                    unit: item.targets.first().cloned(),
                    count: item.targets.len() as u32,
                    hint: "Drop any unit",
                    disabled,
                    on_click: move |_| {
                        on_assign_slot
                            .call(AssignmentTarget::ExistingTarget {
                                item_id,
                            })
                    },
                    on_count: move |v: u32| adjust_vec("targets", v),
                }
            }
            div { class: "mt-3 pt-2 border-t border-neutral-700 flex items-center justify-center gap-2",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500",
                    "Delay after prev"
                }
                input {
                    r#type: "number",
                    value: "{item.start_after.value()}",
                    step: "any",
                    min: "0",
                    disabled,
                    oninput: move |e| {
                        if !disabled {
                            if let Ok(v) = e.value().parse::<f64>() {
                                update_start_after(v);
                            }
                        }
                    },
                    class: if disabled { "w-16 px-2 py-1 bg-neutral-800 border border-neutral-700 rounded text-white text-sm text-center cursor-not-allowed opacity-60" } else { "w-16 px-2 py-1 bg-neutral-800 border border-neutral-700 rounded text-white text-sm text-center focus:outline-none focus:border-blue-500" },
                }
                span { class: "text-[10px] text-neutral-500", "s" }
            }

            if has_estimate {
                div { class: "mt-3 pt-2 border-t border-neutral-700",
                    if item.is_valid() {
                        if stalled {
                            div { class: "text-xs text-orange-400",
                                "Estimated time: ≥{MAX_SOLVER_TIME as u32}s (won’t finish with current economy)"
                            }
                        } else {
                            div {
                                class: "grid grid-cols-2 gap-x-3 gap-y-1 text-xs",
                                div { class: "text-neutral-400",
                                    "Finish:"
                                    span { class: "text-neutral-200 ml-1", "{finish_text}" }
                                }
                                div { class: "text-neutral-400",
                                    "Duration:"
                                    span { class: "text-neutral-200 ml-1", "{duration_text}" }
                                }
                                                        // DeltaLine {
                            //     label: "Mass",
                            //     after: finish_time
                            //         .map(|_| {
                            //             estimate.read().as_ref().unwrap().0.eco.production_per_second_mass.value()
                            //         }),
                            //     delta: delta.mass_prod,
                            // }
                            // DeltaLine {
                            //     label: "Energy",
                            //     after: finish_time
                            //         .map(|_| {
                            //             estimate.read().as_ref().unwrap().0.eco.production_per_second_energy.value()
                            //         }),
                            //     delta: delta.energy_prod,
                            // }
                            // DeltaLine {
                            //     label: "Mass cap",
                            //     after: finish_time.map(|_| estimate.read().as_ref().unwrap().0.eco.mass_storage.value()),
                            //     delta: delta.mass_storage,
                            // }
                            // DeltaLine {
                            //     label: "Energy cap",
                            //     after: finish_time.map(|_| estimate.read().as_ref().unwrap().0.eco.energy_storage.value()),
                            //     delta: delta.energy_storage,
                            // }
                            // DeltaLine {
                            //     label: "Maint",
                            //     after: finish_time
                            //         .map(|_| {
                            //             estimate
                            //                 .read()
                            //                 .as_ref()
                            //                 .unwrap()
                            //                 .0
                            //                 .eco
                            //                 .maintenance_consumption_per_second_energy
                            //                 .value()
                            //         }),
                            //     delta: delta.maintenance,
                            // }
                            }
                        }
                    } else {
                        div { class: "text-xs text-neutral-500 italic",
                            "Add a builder and a target to see the estimate."
                        }
                    }
                }
            }
        }
    }
}

// #[derive(Clone, Copy, Default)]
// struct EcoDelta {
//     mass_prod: f64,
//     energy_prod: f64,
//     mass_storage: f64,
//     energy_storage: f64,
//     maintenance: f64,
// }

#[component]
fn DeltaLine(label: &'static str, after: Option<f64>, delta: f64) -> Element {
    let sign = if delta >= 0.0 { "+" } else { "" };
    let range = after.map(|a| format!("{:.1} → {:.1}", a - delta, a));
    rsx! {
        div { class: "text-neutral-400",
            "{label}:"
            if let Some(range) = range {
                span { class: "text-neutral-200 ml-1", "{range}" }
                span { class: if delta >= 0.0 { "text-emerald-400 ml-1" } else { "text-red-400 ml-1" } }
                "({sign}{delta:.1})"
            } else {
                span { class: "text-neutral-500 ml-1", "—" }
            }
        }
    }
}

fn resize_with_template(
    vec: &mut Vec<crate::types::UnitSummary>,
    new_len: usize,
    template: Option<crate::types::UnitSummary>,
) {
    if vec.len() == new_len {
        return;
    }
    if new_len == 0 || template.is_none() {
        vec.clear();
        return;
    }
    let template = template.unwrap();
    while vec.len() < new_len {
        vec.push(template.clone());
    }
    vec.truncate(new_len);
}
