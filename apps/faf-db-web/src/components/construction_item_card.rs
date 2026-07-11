use dioxus::prelude::*;
use faf_sim::Time;

use crate::components::UnitBlock;
use crate::types::{AssignmentTarget, ConstructionItem, ConstructionPlan};

#[component]
pub fn ConstructionItemCard(
    item: ConstructionItem,
    mut plan: Signal<ConstructionPlan>,
    on_assign_slot: EventHandler<AssignmentTarget>,
) -> Element {
    let item_id = item.id;

    let remove = move |_| {
        plan.with_mut(|p| p.items.retain(|i| i.id != item_id));
    };

    let mut adjust_vec = move |field: &'static str, new_len: u32| {
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
        plan.with_mut(|p| {
            if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                i.start_after = Time::from_raw(value.max(0.0));
            }
        });
    };

    rsx! {
        div { class: "w-full p-3 rounded-lg bg-neutral-800/50 border border-neutral-700 text-sm",
            div { class: "flex items-center justify-between mb-2",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Queue Item" }
                button {
                    class: "px-2 py-0.5 rounded bg-red-900/40 hover:bg-red-900/60 text-red-300 text-xs transition-colors",
                    onclick: remove,
                    "×"
                }
            }
            div { class: "flex flex-col gap-2",
                UnitBlock {
                    label: "Builder",
                    unit: item.builders.first().cloned(),
                    count: item.builders.len() as u32,
                    hint: "Requires build power",
                    on_click: move |_| on_assign_slot.call(AssignmentTarget::ExistingBuilder { item_id }),
                    on_count: move |v: u32| adjust_vec("builders", v),
                }
                UnitBlock {
                    label: "Target",
                    unit: item.targets.first().cloned(),
                    count: item.targets.len() as u32,
                    hint: "Drop any unit",
                    on_click: move |_| on_assign_slot.call(AssignmentTarget::ExistingTarget { item_id }),
                    on_count: move |v: u32| adjust_vec("targets", v),
                }
            }
            div { class: "mt-3 pt-2 border-t border-neutral-700 flex items-center justify-center gap-2",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Start After" }
                input {
                    r#type: "number",
                    value: "{item.start_after.value()}",
                    step: "any",
                    min: "0",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f64>() {
                            update_start_after(v);
                        }
                    },
                    class: "w-16 px-2 py-1 bg-neutral-800 border border-neutral-700 rounded text-white text-sm text-center focus:outline-none focus:border-blue-500",
                }
                span { class: "text-[10px] text-neutral-500", "s" }
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
