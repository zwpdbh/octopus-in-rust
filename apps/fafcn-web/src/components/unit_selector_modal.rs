use dioxus::prelude::*;

use crate::components::{UnitSelector, UnitSummary};

/// What slot the user is picking a unit for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentTarget {
    /// Edit the builder of every action in the half-open index range [start, end).
    ExistingBuilder {
        start: u32,
        end: u32,
    },
    /// Edit the target of every action in the half-open index range [start, end).
    ExistingTarget {
        start: u32,
        end: u32,
    },
    NewBuilder,
    NewTarget,
}

impl AssignmentTarget {
    pub fn accepts(self, unit: &UnitSummary) -> bool {
        match self {
            AssignmentTarget::ExistingBuilder { .. } | AssignmentTarget::NewBuilder => {
                unit.category.as_deref() == Some("Construction - Buildpower")
            }
            _ => true,
        }
    }
}

/// Modal wrapper around `UnitSelector`.
#[component]
pub fn UnitSelectorModal(
    open: bool,
    units: Vec<UnitSummary>,
    target: AssignmentTarget,
    on_select: EventHandler<UnitSummary>,
    on_close: EventHandler<()>,
) -> Element {
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
                    UnitSelector { units: filtered, on_select }
                }
            }
        }
    }
}
