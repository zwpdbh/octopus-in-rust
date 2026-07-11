use dioxus::prelude::*;

use crate::components::{AppHeader, UnitDetail, UnitSelector};
use crate::route::Route;
use crate::types::UnitSummary;

#[component]
pub fn Home() -> Element {
    let units_res = use_context::<Resource<Option<Vec<UnitSummary>>>>();
    let mut selected = use_signal(|| None::<UnitSummary>);

    let units_data = units_res.read().clone();
    match units_data {
        Some(Some(units)) => rsx! {
            div { class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans overflow-hidden select-none",
                AppHeader { active: Route::Home {} }
                div { class: "flex flex-1 overflow-hidden",
                    UnitSelector {
                        units,
                        selected,
                        on_select: move |unit: UnitSummary| {
                            if selected.read().as_ref().map(|s| s.id == unit.id).unwrap_or(false) {
                                selected.set(None);
                            } else {
                                selected.set(Some(unit));
                            }
                        },
                    }
                    div { class: "w-96 shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-auto p-4",
                        UnitDetail { selected }
                    }
                }
            }
        },
        Some(None) => rsx! { "Failed to load units" },
        None => rsx! { "Loading..." },
    }
}
