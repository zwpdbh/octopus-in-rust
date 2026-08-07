use dioxus::prelude::*;
use faf_blueprints::{TechLevel, UnitCostMetrics, UnitEffectEcoMetrics};
use serde::Deserialize;

use super::PortraitButton;

#[derive(Clone, Deserialize, PartialEq)]
pub struct UnitSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub faction: String,
    pub tech_level: TechLevel,
    pub cost: UnitCostMetrics,
    pub eco_effect: UnitEffectEcoMetrics,
    pub category: Option<String>,
    pub kind: Option<String>,
    pub strategic_icon_name: Option<String>,
}

impl UnitSummary {
    /// Build a full blueprint from the summary fields.
    pub fn to_blueprint(&self) -> faf_blueprints::UnitBlueprint {
        faf_blueprints::UnitBlueprint::new(
            self.id.clone(),
            self.name.clone(),
            self.cost,
            self.eco_effect.clone(),
            self.tech_level,
            None,
            None,
            self.strategic_icon_name.clone(),
        )
    }
}

#[component]
pub fn UnitSelectorModal(
    units: Vec<UnitSummary>,
    open: Signal<bool>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let mut search = use_signal(|| String::new());

    if !*open.read() {
        return rsx! {};
    }

    let q = search.read().to_lowercase();
    let filtered: Vec<UnitSummary> = units
        .into_iter()
        .filter(|u| {
            u.id.to_lowercase().contains(&q)
                || u.name.to_lowercase().contains(&q)
                || u.description.to_lowercase().contains(&q)
        })
        .collect();

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/70",
            div { class: "w-full max-w-3xl max-h-[80vh] flex flex-col bg-neutral-900 border border-neutral-700 rounded-lg shadow-xl",
                div { class: "flex items-center justify-between p-4 border-b border-neutral-800",
                    h3 { class: "text-lg font-semibold text-white", "Select Unit" }
                    button {
                        class: "text-neutral-400 hover:text-white",
                        onclick: move |_| open.set(false),
                        "✕"
                    }
                }
                div { class: "p-4",
                    input {
                        class: "w-full px-3 py-2 bg-neutral-800 border border-neutral-700 rounded text-white focus:outline-none focus:border-blue-500",
                        placeholder: "Search by id, name, or description...",
                        value: "{search}",
                        oninput: move |evt| search.set(evt.value()),
                    }
                }
                div { class: "flex-1 overflow-y-auto p-4",
                    if filtered.is_empty() {
                        div { class: "text-neutral-400", "No units match." }
                    } else {
                        div { class: "grid grid-cols-[repeat(auto-fill,minmax(5rem,1fr))] gap-3",
                            for unit in filtered {
                                PortraitButton {
                                    key: "{unit.id}",
                                    unit_id: unit.id.clone(),
                                    label: unit.name.clone(),
                                    selected: false,
                                    on_click: move |_| {
                                        on_select.call(unit.clone());
                                        open.set(false);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
