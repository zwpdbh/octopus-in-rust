use dioxus::prelude::*;
use faf_blueprints::{ConstructionAction, ConstructionPlan, UnitBlueprint};

use crate::components::{AssignmentTarget, UnitBlock, UnitSummary};

#[component]
pub fn ConstructionItemCard(
    index: usize,
    item: ConstructionAction,
    plan: Signal<ConstructionPlan>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let remove = move |_| {
        if !disabled {
            plan.with_mut(|p| {
                let mut queue = p.building_queue().to_vec();
                if index < queue.len() {
                    queue.remove(index);
                    *p = ConstructionPlan::new(p.player_eco().clone(), queue);
                }
            });
        }
    };

    let adjust_builders = move |new_len: u32| {
        if disabled {
            return;
        }
        plan.with_mut(|p| {
            let mut queue = p.building_queue().to_vec();
            if let Some(action) = queue.get_mut(index) {
                let template = action.builders().first().cloned();
                let mut builders = action.builders().to_vec();
                resize_with_template(&mut builders, new_len as usize, template);
                action.set_builders(builders);
            }
            *p = ConstructionPlan::new(p.player_eco().clone(), queue);
        });
    };

    let builder_summary = item.builders().first().map(UnitSummary::from_blueprint);
    let target_summary = Some(UnitSummary::from_blueprint(item.target()));
    let builder_count = item.builders().len() as u32;

    rsx! {
        div { class: "w-full p-3 rounded-lg bg-neutral-800/50 border border-neutral-700 text-sm",
            div { class: "flex items-center justify-between mb-2",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Queue Item #{index + 1}" }
                button {
                    class: if disabled { "px-2 py-0.5 rounded bg-red-900/20 text-red-300/50 text-xs cursor-not-allowed" } else { "px-2 py-0.5 rounded bg-red-900/40 hover:bg-red-900/60 text-red-300 text-xs transition-colors" },
                    disabled,
                    onclick: remove,
                    "x"
                }
            }
            div { class: "flex flex-col gap-2",
                UnitBlock {
                    label: "Builder".to_string(),
                    unit: builder_summary,
                    count: builder_count,
                    hint: "Requires build power".to_string(),
                    disabled,
                    on_click: move |_| on_assign_slot.call(AssignmentTarget::ExistingBuilder { item_id: index as u32 }),
                    on_count: adjust_builders,
                }
                UnitBlock {
                    label: "Target".to_string(),
                    unit: target_summary,
                    count: 1,
                    hint: "Drop any unit".to_string(),
                    disabled,
                    on_click: move |_| on_assign_slot.call(AssignmentTarget::ExistingTarget { item_id: index as u32 }),
                    on_count: move |_| {},
                }
            }
        }
    }
}

fn resize_with_template(
    vec: &mut Vec<UnitBlueprint>,
    new_len: usize,
    template: Option<UnitBlueprint>,
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
