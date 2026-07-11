use dioxus::prelude::*;

use crate::components::UnitBlock;
use crate::types::{AssignmentTarget, UnitSummary};

#[component]
pub fn QueueItemCreator(
    draft_builder: Signal<Option<UnitSummary>>,
    draft_builder_count: Signal<u32>,
    draft_target: Signal<Option<UnitSummary>>,
    draft_target_count: Signal<u32>,
    on_assign_slot: EventHandler<AssignmentTarget>,
    on_save: EventHandler<()>,
    on_clear: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-3",
            h3 { class: "text-sm font-semibold text-white", "New Item" }
            UnitBlock {
                label: "Builder",
                unit: draft_builder.read().clone(),
                count: *draft_builder_count.read(),
                hint: "Requires build power",
                on_click: move |_| on_assign_slot.call(AssignmentTarget::NewBuilder),
                on_count: move |v: u32| draft_builder_count.set(v),
            }
            UnitBlock {
                label: "Target",
                unit: draft_target.read().clone(),
                count: *draft_target_count.read(),
                hint: "Drop any unit",
                on_click: move |_| on_assign_slot.call(AssignmentTarget::NewTarget),
                on_count: move |v: u32| draft_target_count.set(v),
            }
            div { class: "flex items-center gap-2",
                button {
                    class: "flex-1 px-3 py-1.5 text-sm rounded bg-blue-700 hover:bg-blue-600 disabled:bg-neutral-700 disabled:text-neutral-500 text-white transition-colors",
                    disabled: draft_builder.read().is_none() || draft_target.read().is_none(),
                    onclick: move |_| on_save.call(()),
                    "Save"
                }
                button {
                    class: "flex-1 px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    onclick: move |_| on_clear.call(()),
                    "Clear"
                }
            }
        }
    }
}
