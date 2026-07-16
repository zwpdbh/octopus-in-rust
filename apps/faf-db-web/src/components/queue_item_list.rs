use dioxus::prelude::*;

use crate::components::ConstructionItemCard;
use crate::types::{AssignmentTarget, ConstructionPlan};

#[component]
pub fn QueueItemList(
    plan: Signal<ConstructionPlan>,
    plan_estimate: Signal<Option<faf_sim::PlanResult>>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let items = plan.read().items.clone();
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if items.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No items in the queue yet. Use the New Item panel on the left to add one." }
            }
            div { class: "grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-3 content-start",
                for item in items {
                    ConstructionItemCard { item, plan, plan_estimate, disabled, on_assign_slot }
                }
            }
        }
    }
}
