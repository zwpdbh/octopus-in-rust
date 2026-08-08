use dioxus::prelude::*;
use faf_blueprints::ConstructionPlan;

use crate::components::{AssignmentTarget, ConstructionItemCard};

#[component]
pub fn QueueItemList(
    plan: Signal<ConstructionPlan>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let items = plan.read().building_queue().to_vec();
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if items.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No items in the queue yet. Use the New Item panel on the left to add one." }
            }
            div { class: "grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3 content-start",
                for (index, item) in items.into_iter().enumerate() {
                    ConstructionItemCard {
                        key: "{index}",
                        index,
                        item,
                        plan,
                        disabled,
                        on_assign_slot,
                    }
                }
            }
        }
    }
}
