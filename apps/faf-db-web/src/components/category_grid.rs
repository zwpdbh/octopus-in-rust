use std::collections::HashMap;

use dioxus::prelude::*;

use crate::components::PortraitButton;
use crate::types::UnitSummary;
use crate::utils::{CATEGORY_ORDER, FACTION_ORDER};

#[component]
pub fn CategoryGrid(
    units: Vec<UnitSummary>,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let mut by_category: HashMap<String, Vec<UnitSummary>> = HashMap::new();
    for unit in units {
        by_category
            .entry(unit.category.clone())
            .or_default()
            .push(unit);
    }

    let mut ordered: Vec<(String, Vec<UnitSummary>)> = Vec::new();
    for category in CATEGORY_ORDER.iter().copied() {
        if let Some(group) = by_category.remove(category) {
            ordered.push((category.to_string(), group));
        }
    }

    rsx! {
        div {
            class: "flex flex-wrap gap-4 items-start content-start",
            for (category, units) in ordered {
                CategoryPanel { category, units, selected, on_select }
            }
        }
    }
}

#[component]
fn CategoryPanel(
    category: String,
    units: Vec<UnitSummary>,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let all_techs: Vec<&str> = if category == "Experimental" {
        vec!["EXPERIMENTAL"]
    } else {
        vec!["TECH1", "TECH2", "TECH3"]
    };
    let techs: Vec<&str> = all_techs
        .into_iter()
        .filter(|tech| units.iter().any(|u| u.tech == **tech))
        .collect();

    rsx! {
        div {
            class: "border border-neutral-700 rounded-lg bg-neutral-900/80 backdrop-blur-sm p-3 shadow-lg",
            h2 { class: "text-sm font-semibold text-center text-white mb-3 tracking-wide", "{category}" }
            div {
                class: "flex",
                for (i, tech) in techs.iter().enumerate() {
                    div {
                        class: "flex flex-col gap-1.5",
                        class: if i > 0 { "pl-1.5" },
                        class: if i < techs.len() - 1 { "pr-1.5 border-r-2 border-dashed border-white/60" },
                        for faction in FACTION_ORDER.iter().copied() {
                            TechCell {
                                units: units.iter().filter(|u| u.faction == faction && u.tech == *tech).cloned().collect::<Vec<_>>(),
                                faction,
                                selected,
                                on_select,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TechCell(
    units: Vec<UnitSummary>,
    faction: &'static str,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    if units.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            class: "flex flex-wrap gap-1.5",
            for unit in units {
                PortraitButton { unit, faction, selected, on_select }
            }
        }
    }
}
