use dioxus::prelude::*;
use faf_blueprints::TechLevel;
use gloo_net::http::Request;
use std::collections::HashSet;

use crate::components::UnitSummary;
use crate::utils::{
    faction_color, faction_glow_class, tech_level_short, CATEGORY_ORDER, FACTION_ORDER,
};

/// A browsable unit database for the home page.
///
/// Loads the unit list from `/api/units`, lets the user search and filter by
/// faction/kind/tech, and shows a detail panel for the selected unit.
#[component]
pub fn UnitBrowser() -> Element {
    let units = use_resource(move || async move {
        Request::get("http://localhost:3000/api/units")
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json::<Vec<UnitSummary>>()
            .await
            .map_err(|e| e.to_string())
    });
    let mut query = use_signal(String::new);
    let active_factions = use_signal(HashSet::<String>::new);
    let active_kinds = use_signal(HashSet::<String>::new);
    let active_techs = use_signal(HashSet::<String>::new);
    let mut selected_unit = use_signal(|| None::<UnitSummary>);

    let unit_list = match units.read().as_ref() {
        Some(Ok(list)) => list.clone(),
        Some(Err(err)) => {
            return rsx! {
                div { class: "flex items-center justify-center h-full text-red-400",
                    "Failed to load units: {err}"
                }
            };
        }
        None => {
            return rsx! {
                div { class: "flex items-center justify-center h-full text-neutral-400",
                    "Loading units..."
                }
            };
        }
    };

    let filtered: Vec<UnitSummary> = unit_list
        .into_iter()
        .filter(|u| {
            let q = query.read().to_lowercase();
            let text_match = q.is_empty()
                || u.id.to_lowercase().contains(&q)
                || u.name.to_lowercase().contains(&q)
                || u.description.to_lowercase().contains(&q);
            let faction_match = active_factions.read().is_empty()
                || active_factions.read().contains(&u.faction.to_lowercase());
            let kind_match = active_kinds.read().is_empty()
                || u.kind
                    .as_ref()
                    .map(|k| active_kinds.read().contains(k))
                    .unwrap_or(false);
            let tech_match = active_techs.read().is_empty()
                || active_techs.read().contains(tech_level_short(u.tech_level));
            text_match && faction_match && kind_match && tech_match
        })
        .collect();

    let on_select_unit = move |unit: UnitSummary| {
        selected_unit.with_mut(|s| {
            if s.as_ref().map(|sel| sel.id == unit.id).unwrap_or(false) {
                *s = None;
            } else {
                *s = Some(unit);
            }
        });
    };

    rsx! {
        div { class: "flex flex-col h-full bg-neutral-950 text-gray-200 overflow-hidden font-sans select-none",
            header { class: "flex flex-wrap items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                input {
                    class: "flex-1 min-w-[12rem] max-w-sm px-3 py-1.5 bg-neutral-800 border border-neutral-700 rounded text-sm text-white placeholder-neutral-500 focus:outline-none focus:border-blue-500",
                    r#type: "text",
                    placeholder: "Search units...",
                    value: "{query.read()}",
                    oninput: move |e| query.set(e.value()),
                }
                FilterBar {
                    active_factions,
                    active_kinds,
                    active_techs,
                }
            }

            div { class: "flex flex-1 min-h-0 overflow-hidden",
                div { class: "flex-1 overflow-y-auto p-4",
                    CategoryGrid { units: filtered, selected: selected_unit, on_select: on_select_unit }
                }
                div { class: "w-96 shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-y-auto p-4",
                    UnitDetailPanel { unit: selected_unit }
                }
            }
        }
    }
}

#[component]
fn FilterBar(
    active_factions: Signal<HashSet<String>>,
    active_kinds: Signal<HashSet<String>>,
    active_techs: Signal<HashSet<String>>,
) -> Element {
    rsx! {
        div { class: "flex items-center gap-4",
            FilterGroup {
                items: vec!["uef", "cybran", "aeon", "seraphim"],
                active: active_factions,
                icon_dir: "embed_icons",
                extension: "svg",
            }
            FilterGroup {
                items: vec!["Base", "Land", "Air", "Naval"],
                active: active_kinds,
                icon_dir: "ui",
                extension: "png",
            }
            FilterGroup {
                items: vec!["T1", "T2", "T3", "EXP"],
                active: active_techs,
                icon_dir: "ui",
                extension: "png",
            }
        }
    }
}

#[component]
fn FilterGroup(
    items: Vec<&'static str>,
    active: Signal<HashSet<String>>,
    icon_dir: &'static str,
    extension: &'static str,
) -> Element {
    rsx! {
        div { class: "flex items-center gap-1",
            for item in items {
                FilterButton {
                    key: "{item}",
                    item,
                    active,
                    icon_dir,
                    extension,
                }
            }
        }
    }
}

#[component]
fn FilterButton(
    item: &'static str,
    active: Signal<HashSet<String>>,
    icon_dir: &'static str,
    extension: &'static str,
) -> Element {
    let is_active = active.read().contains(item);
    let active_class = if is_active {
        "opacity-100 bg-white/15 ring-1 ring-white/30"
    } else {
        "opacity-75 hover:opacity-100 bg-neutral-800/50 hover:bg-neutral-700/50"
    };
    let src = format!("/{}/{}.{}?v=1", icon_dir, item, extension);

    rsx! {
        button {
            class: "w-8 h-8 p-1 rounded cursor-pointer transition-all {active_class}",
            title: "{item}",
            onclick: move |_| {
                active.with_mut(|set| {
                    if !set.remove(item) {
                        set.insert(item.to_string());
                    }
                });
            },
            img { src: "{src}", alt: "{item}", class: "w-full h-full object-contain" }
        }
    }
}

#[component]
fn CategoryGrid(
    units: Vec<UnitSummary>,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let mut by_category: std::collections::HashMap<String, Vec<UnitSummary>> =
        std::collections::HashMap::new();
    for unit in units {
        if let Some(category) = unit.category.clone() {
            by_category.entry(category).or_default().push(unit);
        }
    }

    let mut ordered: Vec<(String, Vec<UnitSummary>)> = Vec::new();
    for category in CATEGORY_ORDER.iter().copied() {
        if let Some(group) = by_category.remove(category) {
            ordered.push((category.to_string(), group));
        }
    }

    rsx! {
        div { class: "flex flex-wrap gap-4 items-start content-start",
            if ordered.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8 w-full", "No units match the current filters." }
            }
            for (category, units) in ordered {
                CategoryPanel {
                    key: "{category}",
                    category,
                    units,
                    selected,
                    on_select,
                }
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
    if units.is_empty() {
        return rsx! {};
    }

    let techs: Vec<TechLevel> = [TechLevel::T1, TechLevel::T2, TechLevel::T3, TechLevel::T4]
        .into_iter()
        .filter(|tech| units.iter().any(|u| u.tech_level == *tech))
        .collect();

    rsx! {
        div { class: "border border-neutral-700 rounded-lg bg-neutral-900/80 backdrop-blur-sm p-3 shadow-lg",
            h2 { class: "text-sm font-semibold text-center text-white mb-3 tracking-wide", "{category}" }
            div { class: "flex",
                for (i, tech) in techs.iter().enumerate() {
                    div {
                        key: "{tech:?}",
                        class: "flex flex-col gap-1.5",
                        class: if i > 0 { "pl-1.5" },
                        class: if i < techs.len() - 1 { "pr-1.5 border-r-2 border-dashed border-white/60" },
                        h4 { class: "text-[10px] uppercase tracking-wider text-neutral-500 text-center", "{tech:?}" }
                        for faction in FACTION_ORDER.iter().copied() {
                            TechCell {
                                units: units.iter().filter(|u| u.faction.to_lowercase() == faction.to_lowercase() && u.tech_level == *tech).cloned().collect::<Vec<_>>(),
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
        div { class: "flex flex-wrap gap-1.5",
            for unit in units {
                UnitIcon {
                    key: "{unit.id}",
                    unit: unit.clone(),
                    faction: faction.to_string(),
                    selected: selected.read().as_ref().map(|s| s.id == unit.id).unwrap_or(false),
                    on_select,
                }
            }
        }
    }
}

/// Compact square portrait with faction glow and optional strategic icon overlay.
#[component]
fn UnitIcon(
    unit: UnitSummary,
    faction: String,
    selected: bool,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let id = unit.id.clone();
    let name = unit.name.clone();
    let glow = faction_glow_class(&faction);
    let strategic_src = unit
        .strategic_icon_name
        .as_deref()
        .map(|icon| format!("/strategic/{}_{}.png", faction, icon));

    rsx! {
        button {
            class: "relative w-12 h-12 p-[3px] rounded-[5px] bg-black border cursor-pointer transition-transform hover:scale-105 active:scale-[0.99] active:translate-y-px {glow}",
            class: if selected { "ring-2 ring-white" },
            title: "{name}",
            onclick: move |_| on_select.call(unit.clone()),
            img {
                src: "http://localhost:3000/api/portraits/{id.to_ascii_uppercase()}",
                alt: "{name}",
                class: "w-full h-full object-contain block",
            }
            if let Some(src) = strategic_src {
                img {
                    src: "{src}",
                    alt: "",
                    class: "absolute top-0.5 left-0.5 w-3.5 h-3.5 object-contain pointer-events-none",
                }
            }
        }
    }
}

#[component]
fn UnitDetailPanel(unit: Signal<Option<UnitSummary>>) -> Element {
    match unit.read().clone() {
        None => rsx! {
            div { class: "h-full flex items-center justify-center text-neutral-500 text-sm",
                "Select a unit to view details."
            }
        },
        Some(u) => {
            let cost = u.cost;
            let eco = u.eco_effect;
            let color = faction_color(&u.faction);
            let glow = faction_glow_class(&u.faction);
            rsx! {
                div { class: "space-y-4",
                    img {
                        class: "w-full h-40 object-contain rounded-lg border-2 p-1 {glow}",
                        style: "border-color: {color};",
                        src: "http://localhost:3000/api/portraits/{u.id.to_ascii_uppercase()}",
                        alt: "{u.name}",
                    }
                    div {
                        h2 { class: "text-lg font-semibold text-white leading-tight", "{u.name}" }
                        p { class: "text-xs text-neutral-500 font-mono mt-0.5", "{u.id}" }
                        p { class: "text-sm text-neutral-400 mt-1", "{u.faction.to_uppercase()} · {u.tech_level:?}" }
                        div { class: "flex flex-wrap gap-1 mt-2",
                            if let Some(cat) = u.category {
                                span { class: "px-2 py-0.5 text-[10px] uppercase tracking-wide rounded bg-neutral-800 text-neutral-400 border border-neutral-700", "{cat}" }
                            }
                            if let Some(kind) = u.kind {
                                span { class: "px-2 py-0.5 text-[10px] uppercase tracking-wide rounded bg-neutral-800 text-neutral-400 border border-neutral-700", "{kind}" }
                            }
                        }
                    }
                    div { class: "grid grid-cols-2 gap-2 text-sm",
                        div { class: "bg-neutral-800 rounded p-2", "Mass: {cost.mass:.0}" }
                        div { class: "bg-neutral-800 rounded p-2", "Energy: {cost.energy:.0}" }
                        div { class: "bg-neutral-800 rounded p-2", "Build Time: {cost.build_time:.0}" }
                        div { class: "bg-neutral-800 rounded p-2", "Build Power: {eco.build_power:.1}" }
                    }
                    if eco.generate_mass_rate > 0.0 || eco.generate_energy_rate > 0.0 {
                        div { class: "grid grid-cols-2 gap-2 text-sm",
                            if eco.generate_mass_rate > 0.0 {
                                div { class: "bg-neutral-800 rounded p-2", "Mass Income: {eco.generate_mass_rate:.1}" }
                            }
                            if eco.generate_energy_rate > 0.0 {
                                div { class: "bg-neutral-800 rounded p-2", "Energy Income: {eco.generate_energy_rate:.1}" }
                            }
                        }
                    }
                    p { class: "text-sm text-neutral-400", "{u.description}" }
                }
            }
        }
    }
}
