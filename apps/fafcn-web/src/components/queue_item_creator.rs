use dioxus::prelude::*;

use crate::components::{UnitBlock, UnitSummary};
use crate::i18n::{self, Text};

#[component]
pub fn QueueItemCreator(
    draft_builder: Signal<Option<UnitSummary>>,
    draft_builder_count: Signal<u32>,
    draft_target: Signal<Option<UnitSummary>>,
    draft_target_count: Signal<u32>,
    on_assign_slot: EventHandler<crate::components::AssignmentTarget>,
    on_save: EventHandler<()>,
    on_clear: EventHandler<()>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let save_disabled = disabled || draft_builder.read().is_none() || draft_target.read().is_none();
    let t = i18n::use_t();
    let clear_class = if disabled {
        "flex-1 px-3 py-1.5 text-sm rounded bg-neutral-800 text-neutral-500 border border-neutral-700 cursor-not-allowed"
    } else {
        "flex-1 px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors"
    };

    rsx! {
        div { class: "flex flex-col gap-3",
            h3 { class: "text-sm font-semibold text-white", "{t.t(Text::NewItem)}" }
            UnitBlock {
                label: t.t(Text::Builder).to_string(),
                unit: draft_builder.read().clone(),
                count: *draft_builder_count.read(),
                hint: t.t(Text::HintBuildPower).to_string(),
                disabled,
                on_click: move |_| on_assign_slot.call(crate::components::AssignmentTarget::NewBuilder),
                on_count: move |v: u32| draft_builder_count.set(v),
            }
            UnitBlock {
                label: t.t(Text::Target).to_string(),
                unit: draft_target.read().clone(),
                count: *draft_target_count.read(),
                hint: t.t(Text::HintDropAny).to_string(),
                disabled,
                on_click: move |_| on_assign_slot.call(crate::components::AssignmentTarget::NewTarget),
                on_count: move |v: u32| draft_target_count.set(v),
            }
            div { class: "flex items-center gap-2",
                button {
                    class: "flex-1 px-3 py-1.5 text-sm rounded bg-blue-700 hover:bg-blue-600 disabled:bg-neutral-700 disabled:text-neutral-500 text-white transition-colors",
                    disabled: save_disabled,
                    onclick: move |_| {
                        if !save_disabled {
                            on_save.call(());
                        }
                    },
                    "{t.t(Text::Save)}"
                }
                button {
                    class: "{clear_class}",
                    disabled,
                    onclick: move |_| {
                        if !disabled {
                            on_clear.call(());
                        }
                    },
                    "{t.t(Text::Clear)}"
                }
            }
        }
    }
}
