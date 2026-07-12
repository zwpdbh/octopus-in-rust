use dioxus::prelude::*;

use crate::components::UnitSelector;
use crate::types::{AssignmentTarget, UnitSummary};

#[component]
pub fn UnitSelectorModal(
    open: bool,
    units: Vec<UnitSummary>,
    target: AssignmentTarget,
    on_select: EventHandler<UnitSummary>,
    on_close: EventHandler<()>,
) -> Element {
    let modal_selected = use_signal(|| None::<UnitSummary>);
    if !open {
        return rsx! {};
    }
    let filtered: Vec<UnitSummary> = units.into_iter().filter(|u| target.accepts(u)).collect();

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/70",
            onclick: move |_| on_close.call(()),
            div { class: "w-[900px] h-[80vh] bg-neutral-900 rounded-lg border border-neutral-700 shadow-2xl overflow-hidden flex flex-col",
                onclick: move |e| e.stop_propagation(),
                div { class: "flex items-center justify-between px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                    h3 { class: "text-sm font-semibold text-white", "Select Unit" }
                    button {
                        class: "px-2 py-1 text-lg leading-none text-neutral-400 hover:text-white transition-colors",
                        onclick: move |_| on_close.call(()),
                        "×"
                    }
                }
                div { class: "flex flex-col flex-1 overflow-hidden",
                    UnitSelector { units: filtered, selected: modal_selected, on_select }
                }
            }
        }
    }
}
