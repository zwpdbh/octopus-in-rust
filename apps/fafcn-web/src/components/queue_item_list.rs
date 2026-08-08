use dioxus::prelude::*;
use faf_blueprints::{ConstructionAction, ConstructionPlan};

use crate::components::{AssignmentTarget, ConstructionItemCard};

/// Group consecutive identical construction actions into half-open index ranges.
fn group_identical_actions(queue: &[ConstructionAction]) -> Vec<(usize, usize)> {
    let mut groups = Vec::new();
    if queue.is_empty() {
        return groups;
    }

    let mut start = 0;
    for i in 1..queue.len() {
        if queue[i] != queue[start] {
            groups.push((start, i));
            start = i;
        }
    }
    groups.push((start, queue.len()));
    groups
}

#[component]
pub fn QueueItemList(
    plan: Signal<ConstructionPlan>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let items = plan.read().building_queue().to_vec();
    let groups = group_identical_actions(&items);
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if items.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No items in the queue yet. Use the New Item panel on the left to add one." }
            }
            div { class: "grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-2 content-start",
                for (start , end) in groups {
                    ConstructionItemCard {
                        key: "{start}",
                        start_index: start,
                        end_index: end,
                        item: items[start].clone(),
                        plan,
                        disabled,
                        on_assign_slot,
                    }
                }
            }
        }
    }
}
