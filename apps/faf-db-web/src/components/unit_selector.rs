use std::collections::HashSet;

use dioxus::prelude::*;

use crate::components::{CategoryGrid, FilterBar};
use crate::types::UnitSummary;
use crate::utils::{tech_short, FACTION_ORDER};

#[component]
pub fn UnitSelector(
    units: Vec<UnitSummary>,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let mut query = use_signal(String::new);
    let active_factions = use_signal(HashSet::<String>::new);
    let active_kinds = use_signal(HashSet::<String>::new);
    let active_techs = use_signal(HashSet::<String>::new);

    let allowed_factions: HashSet<String> =
        FACTION_ORDER.iter().map(|f| f.to_lowercase()).collect();
    let filtered: Vec<UnitSummary> = units
        .into_iter()
        .filter(|u| allowed_factions.contains(&u.faction.to_lowercase()))
        .filter(|u| {
            let q = query.read().to_lowercase();
            let text_match = q.is_empty()
                || u.id.to_lowercase().contains(&q)
                || u.display_name.to_lowercase().contains(&q);
            let faction_match = active_factions.read().is_empty()
                || active_factions.read().contains(&u.faction.to_lowercase());
            let kind_match =
                active_kinds.read().is_empty() || active_kinds.read().contains(&u.kind);
            let tech_match = active_techs.read().is_empty()
                || active_techs.read().contains(&tech_short(&u.tech));
            text_match && faction_match && kind_match && tech_match
        })
        .collect();

    rsx! {
        div { class: "flex flex-col flex-1 overflow-hidden",
            header {
                class: "flex flex-wrap items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                input {
                    r#type: "text",
                    placeholder: "Search units...",
                    value: "{query.read()}",
                    oninput: move |e| query.set(e.value().to_string()),
                    class: "flex-1 max-w-sm px-3 py-1.5 bg-neutral-800 border border-neutral-700 rounded text-sm text-white placeholder-neutral-500 focus:outline-none focus:border-blue-500",
                }
                FilterBar {
                    active_factions,
                    active_kinds,
                    active_techs,
                }
            }
            div {
                class: "flex-1 overflow-auto p-4",
                if filtered.is_empty() {
                    div { class: "text-neutral-500 text-sm text-center py-8", "No units match the current filters." }
                }
                CategoryGrid { units: filtered, selected, on_select }
            }
        }
    }
}
