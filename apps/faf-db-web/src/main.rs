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

const CATEGORY_ORDER: &[&str] = &[
    "Land",
    "Naval",
    "Structures - Weapons",
    "Structures - Support",
    "Structures - Intelligence",
    "Air",
    "Structures - Factories",
    "Construction - Buildpower",
    "Structures - Economy",
    "Experimental",
];

const FACTION_ORDER: &[&str] = &["UEF", "Cybran", "Aeon", "Seraphim"];
const FACTION_COLORS: &[&str] = &["#1e90ff", "#dc143c", "#32cd32", "#ffd700"];

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
                div {
                    style: "display: flex; flex-direction: column; height: 100vh; color: #ddd; background: #0b0b0b; font-family: sans-serif;",
                    header {
                        style: "padding: 12px 16px; border-bottom: 1px solid #333; display: flex; align-items: center; gap: 12px;",
                        h1 { style: "margin: 0; font-size: 18px;", "FAF Unit Database" }
                        input {
                            r#type: "text",
                            placeholder: "Search units...",
                            value: "{query.read()}",
                            oninput: move |e| query.set(e.value().to_string()),
                            style: "flex: 1; max-width: 300px; padding: 6px 10px; background: #222; color: #ddd; border: 1px solid #444; border-radius: 4px;",
                        }
                    }
                    div {
                        style: "flex: 1; display: flex; overflow: hidden;",
                        div {
                            style: "flex: 1; overflow: auto; padding: 12px;",
                            CategoryGrid { units: filtered, selected }
                        }
                        div {
                            style: "width: 360px; border-left: 1px solid #333; overflow: auto; padding: 12px; background: #141414;",
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
            style: "display: flex; flex-wrap: wrap; gap: 12px; align-items: flex-start;",
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
        vec!["TECH1", "TECH2", "TECH3", "TECH4"]
    };

    rsx! {
        div {
            style: "border: 1px solid #444; border-radius: 6px; background: #161616; padding: 10px; min-width: 280px;",
            h2 { style: "margin: 0 0 10px; font-size: 14px; text-align: center; color: #fff;", "{category}" }
            div {
                style: "display: flex; gap: 10px;",
                for (i, tech) in techs.iter().enumerate() {
                    div {
                        style: "display: flex; flex-direction: column; gap: 6px;",
                        for faction in FACTION_ORDER.iter().copied() {
                            TechCell {
                                units: units.iter().filter(|u| u.faction == faction && u.tech == *tech).cloned().collect::<Vec<_>>(),
                                faction,
                                selected
                            }
                        }
                    }
                    if i < techs.len() - 1 {
                        div { style: "width: 1px; background: #444; margin: 0 2px;", }
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
) -> Element {
    let color = faction_color(faction);
    rsx! {
        div {
            style: "display: flex; flex-wrap: wrap; gap: 2px; width: 96px; min-height: 22px;",
            for unit in units {
                PortraitButton { unit, color, selected }
            }
        }
    }
}

#[component]
fn PortraitButton(
    unit: UnitSummary,
    color: &'static str,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    let id = unit.id.clone();
    let name = unit.display_name.clone();
    rsx! {
        button {
            style: "padding: 0; border: 1.5px solid {color}; background: #000; cursor: pointer; width: 22px; height: 22px;",
            onclick: move |_| selected.set(Some(unit.clone())),
            img {
                src: "/api/portraits/{id}.png",
                alt: "{name}",
                style: "width: 100%; height: 100%; object-fit: contain; display: block;",
            }
        }
    }
}

#[component]
fn UnitDetail(selected: Signal<Option<UnitSummary>>) -> Element {
    match selected.read().clone() {
        Some(unit) => rsx! {
            div {
                h2 { style: "margin-top: 0; font-size: 16px;", "{unit.display_name}" }
                p { "ID: {unit.id}" }
                p { "Faction: {unit.faction}" }
                p { "Tech: {unit.tech}" }
                p { "Category: {unit.category}" }
                img {
                    src: "/api/portraits/{unit.id}.png",
                    alt: "{unit.display_name}",
                    style: "width: 128px; height: 128px; border: 2px solid {faction_color(&unit.faction)};",
                }
            }
        },
        None => rsx! {
            div { style: "color: #888;", "Select a unit to view details." }
        },
    }
}

fn faction_color(faction: &str) -> &'static str {
    FACTION_ORDER
        .iter()
        .position(|f| f.eq_ignore_ascii_case(faction))
        .map(|i| FACTION_COLORS[i])
        .unwrap_or("#888")
}
