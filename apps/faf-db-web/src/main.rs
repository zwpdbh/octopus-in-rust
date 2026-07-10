use dioxus::prelude::*;
use gloo_net::http::Request;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct UnitSummary {
    id: String,
    display_name: String,
    faction: String,
    tech: String,
    category: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct UnitDetailData {
    id: String,
    description: String,
    #[serde(default)]
    name_zh: Option<String>,
    #[serde(default)]
    description_zh: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    general: Option<GeneralDetail>,
    #[serde(default)]
    economy: Option<EconomyDetail>,
    #[serde(default)]
    defense: Option<DefenseDetail>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct GeneralDetail {
    #[serde(default)]
    unit_name: Option<String>,
    #[serde(default)]
    faction_name: Option<String>,
    #[serde(default)]
    tech_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct EconomyDetail {
    #[serde(default)]
    build_cost_energy: Option<f64>,
    #[serde(default)]
    build_cost_mass: Option<f64>,
    #[serde(default)]
    build_time: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
struct DefenseDetail {
    #[serde(default)]
    max_health: Option<f64>,
}

const CATEGORY_ORDER: &[&str] = &[
    "Land",
    "Air",
    "Naval",
    "Structures - Factories",
    "Structures - Economy",
    "Structures - Weapons",
    "Structures - Support",
    "Structures - Intelligence",
    "Construction - Buildpower",
    "Experimental",
];

const FACTION_ORDER: &[&str] = &["UEF", "Cybran", "Aeon", "Seraphim"];

fn main() {
    console_error_panic_hook::set_once();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let units = use_resource(|| async move {
        let response = Request::get("/api/units").send().await.ok()?;
        response.json::<Vec<UnitSummary>>().await.ok()
    });

    let selected = use_signal(|| None::<UnitSummary>);
    let mut query = use_signal(|| String::new());

    let units_data = units.read().clone();
    match units_data {
        Some(Some(units)) => {
            let filtered: Vec<UnitSummary> = units
                .into_iter()
                .filter(|u| {
                    let q = query.read().to_lowercase();
                    q.is_empty()
                        || u.id.to_lowercase().contains(&q)
                        || u.display_name.to_lowercase().contains(&q)
                })
                .collect();
            rsx! {
                document::Stylesheet { href: asset!("/assets/tailwind.css") }
                div {
                    class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans",
                    header {
                        class: "flex items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                        h1 { class: "text-lg font-semibold text-white tracking-wide", "FAF Unit Database" }
                        input {
                            r#type: "text",
                            placeholder: "Search units...",
                            value: "{query.read()}",
                            oninput: move |e| query.set(e.value().to_string()),
                            class: "flex-1 max-w-sm px-3 py-1.5 bg-neutral-800 border border-neutral-700 rounded text-sm text-white placeholder-neutral-500 focus:outline-none focus:border-blue-500",
                        }
                    }
                    div {
                        class: "flex flex-1 overflow-hidden",
                        div {
                            class: "flex-1 overflow-auto p-4",
                            CategoryGrid { units: filtered, selected }
                        }
                        div {
                            class: "w-96 shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-auto p-4",
                            UnitDetail { selected }
                        }
                    }
                }
            }
        }
        Some(None) => rsx! { "Failed to load units" },
        None => rsx! { "Loading..." },
    }
}

#[component]
fn CategoryGrid(units: Vec<UnitSummary>, selected: Signal<Option<UnitSummary>>) -> Element {
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
                CategoryPanel { category, units, selected }
            }
        }
    }
}

#[component]
fn CategoryPanel(
    category: String,
    units: Vec<UnitSummary>,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    let techs: Vec<&str> = if category == "Experimental" {
        vec!["EXPERIMENTAL"]
    } else {
        vec!["TECH1", "TECH2", "TECH3"]
    };

    rsx! {
        div {
            class: "border border-neutral-700 rounded-lg bg-neutral-900/80 backdrop-blur-sm p-3 min-w-[360px] shadow-lg",
            h2 { class: "text-sm font-semibold text-center text-white mb-3 tracking-wide", "{category}" }
            div {
                class: "flex gap-1",
                for (i, tech) in techs.iter().enumerate() {
                    div {
                        class: "flex flex-col gap-1 min-w-[108px]",
                        div { class: "text-[10px] text-center text-neutral-500 uppercase tracking-wider mb-1", "{tech_label(tech)}" }
                        for faction in FACTION_ORDER.iter().copied() {
                            TechCell {
                                units: units.iter().filter(|u| u.faction == faction && u.tech == *tech).cloned().collect::<Vec<_>>(),
                                faction,
                                selected
                            }
                        }
                    }
                    if i < techs.len() - 1 {
                        div { class: "w-px bg-neutral-800 mx-1", }
                    }
                }
            }
        }
    }
}

fn tech_label(tech: &str) -> &str {
    match tech {
        "TECH1" => "T1",
        "TECH2" => "T2",
        "TECH3" => "T3",
        "TECH4" => "T4",
        "EXPERIMENTAL" => "EXP",
        _ => tech,
    }
}

#[component]
fn TechCell(
    units: Vec<UnitSummary>,
    faction: &'static str,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    rsx! {
        div {
            class: "flex flex-wrap gap-[3px] min-h-[54px] p-1 rounded bg-neutral-950/50",
            for unit in units {
                PortraitButton { unit, faction, selected }
            }
        }
    }
}

#[component]
fn PortraitButton(
    unit: UnitSummary,
    faction: &'static str,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    let id = unit.id.clone();
    let name = unit.display_name.clone();
    let glow = faction_glow_class(faction);
    let is_selected = selected.read().as_ref().map(|s| s.id == unit.id).unwrap_or(false);

    rsx! {
        button {
            class: "w-12 h-12 p-[3px] rounded-[5px] bg-black border cursor-pointer transition-transform hover:scale-105 active:scale-[0.99] active:translate-y-px {glow}",
            class: if is_selected { "ring-2 ring-white" },
            title: "{name}",
            onclick: move |_| selected.set(Some(unit.clone())),
            img {
                src: "/api/portraits/{id}.png",
                alt: "{name}",
                class: "w-full h-full object-contain block",
            }
        }
    }
}

#[component]
fn UnitDetail(selected: Signal<Option<UnitSummary>>) -> Element {
    let detail = use_resource(move || async move {
        let summary = selected.read().clone()?;
        Request::get(&format!("/api/units/{}", summary.id))
            .send()
            .await
            .ok()?
            .json::<UnitDetailData>()
            .await
            .ok()
    });

    match selected.read().clone() {
        Some(summary) => {
            let color = faction_color(&summary.faction);
            let glow = faction_glow_class(&summary.faction);
            let detail_data = detail.read().clone();
            rsx! {
                div { class: "space-y-4",
                    div { class: "flex items-start gap-4",
                        img {
                            src: "/api/portraits/{summary.id}.png",
                            alt: "{summary.display_name}",
                            class: "w-24 h-24 object-contain rounded-lg border-2 p-1 {glow}",
                            style: "border-color: {color};",
                        }
                        div { class: "flex-1 min-w-0",
                            h2 { class: "text-lg font-semibold text-white leading-tight", "{summary.display_name}" }
                            p { class: "text-xs text-neutral-500 font-mono mt-0.5", "{summary.id}" }
                            p { class: "text-sm text-neutral-400 mt-1", "{summary.faction} · {summary.tech}" }
                        }
                    }

                    match detail_data {
                        Some(Some(d)) => {
                            let health = d.defense.as_ref().and_then(|x| x.max_health).map(|v| format!("{v:.0}"));
                            let mass = d.economy.as_ref().and_then(|x| x.build_cost_mass).map(|v| format!("{v:.0}"));
                            let energy = d.economy.as_ref().and_then(|x| x.build_cost_energy).map(|v| format!("{v:.0}"));
                            let build_time = d.economy.as_ref().and_then(|x| x.build_time).map(|v| format!("{v:.0}"));
                            rsx! {
                                div { class: "space-y-3",
                                    if !d.description.is_empty() {
                                        p { class: "text-sm text-neutral-300 italic", "{d.description}" }
                                    }
                                    div { class: "grid grid-cols-2 gap-2 text-sm",
                                        Stat { label: "Health", value: health }
                                        Stat { label: "Mass", value: mass }
                                        Stat { label: "Energy", value: energy }
                                        Stat { label: "Build Time", value: build_time }
                                    }
                                    if !d.categories.is_empty() {
                                        div { class: "flex flex-wrap gap-1",
                                            for cat in d.categories {
                                                span { class: "px-2 py-0.5 text-[10px] uppercase tracking-wide rounded bg-neutral-800 text-neutral-400 border border-neutral-700", "{cat}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Some(None) => rsx! { div { class: "text-red-400 text-sm", "Failed to load details." } },
                        None => rsx! { div { class: "text-neutral-500 text-sm", "Loading details..." } },
                    }
                }
            }
        }
        None => rsx! {
            div { class: "h-full flex items-center justify-center text-neutral-500 text-sm",
                "Select a unit to view details."
            }
        },
    }
}

#[component]
fn Stat(label: String, value: Option<String>) -> Element {
    rsx! {
        div { class: "flex flex-col px-3 py-2 rounded bg-neutral-800/50 border border-neutral-800",
            span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
            span { class: "text-white font-medium",
                {value.as_deref().unwrap_or("—")}
            }
        }
    }
}


fn faction_color(faction: &str) -> &'static str {
    match faction.to_lowercase().as_str() {
        "uef" => "#2d78b2",
        "cybran" => "#df2d0e",
        "aeon" => "#19b340",
        "seraphim" => "#fcb419",
        _ => "#888",
    }
}

/// Tailwind-aware portrait glow class. The returned literals are scanned by
/// Tailwind so the arbitrary values end up in the generated CSS.
fn faction_glow_class(faction: &str) -> &'static str {
    match faction.to_lowercase().as_str() {
        "uef" => "border-[rgba(148,193,227,0.33)] shadow-[inset_0_0_4px_rgba(70,174,255,0.43)] bg-[rgba(45,120,178,0.13)]",
        "cybran" => "border-[rgba(247,157,142,0.3)] shadow-[inset_0_0_4px_rgba(255,109,84,0.4)] bg-[rgba(223,45,14,0.1)]",
        "aeon" => "border-[rgba(120,236,150,0.33)] shadow-[inset_0_0_4px_rgba(51,255,103,0.43)] bg-[rgba(25,179,64,0.13)]",
        "seraphim" => "border-[rgba(253,229,176,0.3)] shadow-[inset_0_0_4px_rgba(255,213,124,0.4)] bg-[rgba(252,180,25,0.1)]",
        _ => "border-neutral-600",
    }
}
