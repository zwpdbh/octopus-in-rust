use dioxus::prelude::*;
use gloo_net::http::Request;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct UnitSummary {
    id: String,
    display_name: String,
    faction: String,
    tech: String,
    category: String,
    #[serde(default)]
    strategic_icon_name: Option<String>,
    kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct EcoSettings {
    mass_income: f64,
    energy_income: f64,
    mass_storage: f64,
    energy_storage: f64,
}

impl Default for EcoSettings {
    fn default() -> Self {
        Self {
            mass_income: 1.0,
            energy_income: 20.0,
            mass_storage: 650.0,
            energy_storage: 4000.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ConstructionItem {
    id: u32,
    builder: Option<UnitSummary>,
    builder_count: u32,
    target: Option<UnitSummary>,
    target_count: u32,
    start_after_seconds: f64,
}

impl ConstructionItem {
    fn new(id: u32) -> Self {
        Self {
            id,
            builder: None,
            builder_count: 1,
            target: None,
            target_count: 1,
            start_after_seconds: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct ConstructionPlan {
    eco: EcoSettings,
    items: Vec<ConstructionItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppView {
    Builder,
    Simulation,
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
    let active_factions = use_signal(|| HashSet::<String>::new());
    let active_kinds = use_signal(|| HashSet::<String>::new());
    let active_techs = use_signal(|| HashSet::<String>::new());
    let mut split_pct = use_signal(|| 50.0_f64);
    let mut drag_start = use_signal(|| None::<(f64, f64)>);
    let plan = use_signal(|| load_plan_from_storage().unwrap_or_default());
    let mut view = use_signal(|| AppView::Builder);

    let units_data = units.read().clone();
    match units_data {
        Some(Some(units)) => {
            let filtered: Vec<UnitSummary> = units
                .into_iter()
                .filter(|u| {
                    let q = query.read().to_lowercase();
                    let text_match = q.is_empty()
                        || u.id.to_lowercase().contains(&q)
                        || u.display_name.to_lowercase().contains(&q);
                    let faction_match = active_factions.read().is_empty()
                        || active_factions.read().contains(&u.faction);
                    let kind_match = active_kinds.read().is_empty()
                        || active_kinds.read().contains(&u.kind);
                    let tech_match = active_techs.read().is_empty()
                        || active_techs.read().contains(&tech_short(&u.tech));
                    text_match && faction_match && kind_match && tech_match
                })
                .collect();
            match *view.read() {
                AppView::Builder => rsx! {
                    document::Stylesheet { href: asset!("/assets/tailwind.css") }
                    div {
                        class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans overflow-hidden select-none",
                        class: if drag_start.read().is_some() { "cursor-row-resize" },
                        onmousemove: move |e| {
                            if let Some((start_y, start_pct)) = *drag_start.read() {
                                let height = web_sys::window()
                                    .and_then(|w| w.inner_height().ok())
                                    .and_then(|h| h.as_f64())
                                    .unwrap_or(800.0);
                                let delta_pct = (e.data.client_coordinates().y - start_y) / height * 100.0;
                                split_pct.set((start_pct + delta_pct).clamp(20.0, 80.0));
                            }
                        },
                        onmouseup: move |_| drag_start.set(None),
                        onmouseleave: move |_| drag_start.set(None),
                        // Top half: header + unit grid
                        div {
                            class: "flex flex-col border-b border-neutral-800",
                            style: "height: {split_pct}vh;",
                            header {
                                class: "flex flex-wrap items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                                h1 { class: "text-lg font-semibold text-white tracking-wide", "FAF Unit Database" }
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
                                CategoryGrid { units: filtered, selected }
                            }
                        }
                        // Draggable divider
                        div {
                            class: "h-2 bg-neutral-800 hover:bg-blue-600 flex items-center justify-center transition-colors shrink-0",
                            class: if drag_start.read().is_some() { "bg-blue-600" },
                            onmousedown: move |e| drag_start.set(Some((e.data.client_coordinates().y, split_pct()))),
                            div { class: "w-10 h-1 bg-neutral-500 rounded-full", }
                        }
                        // Bottom half: construction simulator + unit detail
                        div {
                            class: "flex overflow-hidden",
                            style: "height: {100.0 - *split_pct.read()}vh;",
                            div {
                                class: "flex-1 overflow-hidden flex flex-col bg-neutral-900/30",
                                div {
                                    class: "flex-1 overflow-auto p-4",
                                    div {
                                        class: "flex flex-col lg:flex-row gap-4 h-full",
                                        EcoPanel { plan }
                                        ConstructionQueue { plan, selected }
                                    }
                                }
                                div {
                                    class: "flex items-center justify-end gap-3 px-4 py-2 border-t border-neutral-800 bg-neutral-900/50 shrink-0",
                                    button {
                                        class: "px-4 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                                        onclick: move |_| save_plan_to_storage(&plan.read()),
                                        "Save Plan"
                                    }
                                    button {
                                        class: "px-4 py-1.5 text-sm rounded bg-blue-700 hover:bg-blue-600 text-white transition-colors",
                                        onclick: move |_| view.set(AppView::Simulation),
                                        "Begin Simulation"
                                    }
                                }
                            }
                            div {
                                class: "w-96 shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-auto p-4",
                                UnitDetail { selected }
                            }
                        }
                    }
                },
                AppView::Simulation => rsx! {
                    SimulationPage {
                        plan: plan.read().clone(),
                        on_back: move |_| view.set(AppView::Builder),
                    }
                },
            }
        }
        Some(None) => rsx! { "Failed to load units" },
        None => rsx! { "Loading..." },
    }
}

fn tech_short(tech: &str) -> String {
    match tech {
        "TECH1" => "T1",
        "TECH2" => "T2",
        "TECH3" => "T3",
        "TECH4" | "EXPERIMENTAL" => "EXP",
        _ => tech,
    }
    .to_string()
}

const PLAN_STORAGE_KEY: &str = "faf-db-construction-plan-v3";

fn save_plan_to_storage(plan: &ConstructionPlan) {
    if let Ok(json) = serde_json::to_string(plan) {
        let _ = web_sys::window()
            .and_then(|w| w.local_storage().ok()?)
            .map(|storage| storage.set_item(PLAN_STORAGE_KEY, &json));
    }
}

fn load_plan_from_storage() -> Option<ConstructionPlan> {
    let json = web_sys::window()
        .and_then(|w| w.local_storage().ok()?)
        .and_then(|storage| storage.get_item(PLAN_STORAGE_KEY).ok()?)?;
    serde_json::from_str(&json).ok()
}

#[component]
fn FilterBar(
    active_factions: Signal<HashSet<String>>,
    active_kinds: Signal<HashSet<String>>,
    active_techs: Signal<HashSet<String>>,
) -> Element {
    rsx! {
        div {
            class: "flex items-center gap-4",
            FilterGroup {
                items: vec!["uef", "cybran", "aeon", "seraphim", "nomads"],
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
        div {
            class: "flex items-center gap-1",
            for item in items {
                FilterButton {
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
    let title = item.to_string();
    let src = format!("/{}/{}.{}" , icon_dir, item, extension);
    let active_class = if is_active { "opacity-100 bg-white/10" } else { "opacity-40 grayscale hover:opacity-75 hover:grayscale-[0.5]" };

    rsx! {
        button {
            class: "w-8 h-8 p-1 rounded cursor-pointer transition-all {active_class}",
            title: "{title}",
            onclick: move |_| {
                let mut set = active.write();
                if set.contains(item) {
                    set.remove(item);
                } else {
                    set.insert(item.to_string());
                }
            },
            img {
                src: "{src}",
                alt: "{title}",
                class: "w-full h-full object-contain",
            }
        }
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
                                selected
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
) -> Element {
    if units.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            class: "flex flex-wrap gap-1.5",
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
    let strategic_src = unit.strategic_icon_name.as_deref().map(|icon_name| {
        format!("/strategic/{}_{}.png", faction, icon_name)
    });

    rsx! {
        button {
            class: "relative w-12 h-12 p-[3px] rounded-[5px] bg-black border cursor-pointer transition-transform hover:scale-105 active:scale-[0.99] active:translate-y-px {glow}",
            class: if is_selected { "ring-2 ring-white" },
            title: "{name}",
            onclick: move |_| {
                if is_selected {
                    selected.set(None);
                } else {
                    selected.set(Some(unit.clone()));
                }
            },
            img {
                src: "/api/portraits/{id}.png",
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


#[component]
fn EcoPanel(mut plan: Signal<ConstructionPlan>) -> Element {
    fn update<F: FnOnce(&mut EcoSettings)>(mut plan: Signal<ConstructionPlan>, f: F) {
        plan.with_mut(|p| f(&mut p.eco));
    }

    rsx! {
        div {
            class: "flex flex-col gap-3 min-w-[260px] p-3 rounded-lg border border-neutral-700 bg-neutral-900/80",
            h3 { class: "text-sm font-semibold text-white", "Eco Settings" }
            SliderField {
                label: "Mass Income",
                value: plan.read().eco.mass_income,
                min: 1.0,
                max: 200.0,
                unit: "",
                on_change: move |v: f64| update(plan, |eco| eco.mass_income = v.clamp(1.0, 200.0)),
            }
            SliderField {
                label: "Energy Income",
                value: plan.read().eco.energy_income,
                min: 20.0,
                max: 2000.0,
                unit: "",
                on_change: move |v: f64| update(plan, |eco| eco.energy_income = v.clamp(20.0, 2000.0)),
            }
            SliderField {
                label: "Mass Storage",
                value: plan.read().eco.mass_storage,
                min: 0.0,
                max: 650.0,
                unit: "",
                on_change: move |v: f64| update(plan, |eco| eco.mass_storage = v.clamp(0.0, 650.0)),
            }
            SliderField {
                label: "Energy Storage",
                value: plan.read().eco.energy_storage,
                min: 0.0,
                max: 4000.0,
                unit: "",
                on_change: move |v: f64| update(plan, |eco| eco.energy_storage = v.clamp(0.0, 4000.0)),
            }
        }
    }
}

#[component]
fn SliderField(
    label: String,
    value: f64,
    min: f64,
    max: f64,
    unit: String,
    on_change: EventHandler<f64>,
) -> Element {
    rsx! {
        div { class: "text-sm",
            div { class: "flex items-center justify-between",
                span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
                span { class: "text-xs text-neutral-300", "{value:.0}{unit}" }
            }
            input {
                r#type: "range",
                min: "{min}",
                max: "{max}",
                value: "{value}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        on_change.call(v);
                    }
                },
                class: "w-full h-2 mt-1 bg-neutral-700 rounded-lg appearance-none cursor-pointer accent-blue-500",
            }
        }
    }
}

#[component]
fn ConstructionQueue(
    plan: Signal<ConstructionPlan>,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    let items = plan.read().items.clone();

    rsx! {
        div {
            class: "flex-1 flex flex-col min-w-[320px] p-3 rounded-lg border border-neutral-700 bg-neutral-900/80 overflow-hidden",
            div { class: "flex items-center justify-between mb-3",
                h3 { class: "text-sm font-semibold text-white", "Construction Queue" }
                button {
                    class: "w-7 h-7 flex items-center justify-center rounded bg-blue-700 hover:bg-blue-600 text-white text-lg leading-none transition-colors",
                    title: "Add construction",
                    onclick: move |_| {
                        let next_id = plan.read().items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                        plan.write().items.push(ConstructionItem::new(next_id));
                    },
                    "+"
                }
            }
            div {
                class: "flex-1 overflow-auto space-y-2 pr-1",
                if items.is_empty() {
                    div { class: "text-neutral-500 text-sm text-center py-8", "Click + to add a construction step." }
                }
                for item in items {
                    ConstructionItemCard { item, plan, selected }
                }
            }
        }
    }
}

#[component]
fn ConstructionItemCard(
    item: ConstructionItem,
    mut plan: Signal<ConstructionPlan>,
    selected: Signal<Option<UnitSummary>>,
) -> Element {
    let item_id = item.id;

    let set_builder = move |_| {
        if let Some(unit) = selected.read().clone() {
            plan.with_mut(|p| {
                if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                    i.builder = Some(unit);
                }
            });
        }
    };

    let set_target = move |_| {
        if let Some(unit) = selected.read().clone() {
            plan.with_mut(|p| {
                if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                    i.target = Some(unit);
                }
            });
        }
    };

    let remove = move |_| {
        plan.with_mut(|p| p.items.retain(|i| i.id != item_id));
    };

    let mut update_count = move |field: &'static str, value: f64| {
        plan.with_mut(|p| {
            if let Some(i) = p.items.iter_mut().find(|i| i.id == item_id) {
                match field {
                    "builder" => i.builder_count = value.max(1.0) as u32,
                    "target" => i.target_count = value.max(1.0) as u32,
                    "start" => i.start_after_seconds = value.max(0.0),
                    _ => {}
                }
            }
        });
    };

    rsx! {
        div {
            class: "p-2 rounded bg-neutral-800/50 border border-neutral-700 text-sm",
            div { class: "flex items-center gap-3",
                // Builder
                div { class: "flex flex-col gap-1",
                    span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Builder" }
                    UnitSlot {
                        unit: item.builder.clone(),
                        count: item.builder_count,
                        on_set: set_builder,
                        on_count: move |v| update_count("builder", v),
                    }
                }
                // Target
                div { class: "flex flex-col gap-1 flex-1",
                    span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Unit" }
                    UnitSlot {
                        unit: item.target.clone(),
                        count: item.target_count,
                        on_set: set_target,
                        on_count: move |v| update_count("target", v),
                    }
                }
                // Start time
                div { class: "flex flex-col gap-1 w-24",
                    span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "Start After" }
                    input {
                        r#type: "number",
                        value: "{item.start_after_seconds}",
                        step: "any",
                        min: "0",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<f64>() {
                                update_count("start", v);
                            }
                        },
                        class: "px-2 py-1 bg-neutral-800 border border-neutral-700 rounded text-white text-sm focus:outline-none focus:border-blue-500",
                    }
                    span { class: "text-[10px] text-neutral-500", "seconds" }
                }
                // Remove
                button {
                    class: "self-start px-2 py-1 rounded bg-red-900/40 hover:bg-red-900/60 text-red-300 text-xs transition-colors",
                    onclick: remove,
                    "×"
                }
            }
        }
    }
}

#[component]
fn UnitSlot(
    unit: Option<UnitSummary>,
    count: u32,
    on_set: EventHandler<()>,
    on_count: EventHandler<f64>,
) -> Element {
    rsx! {
        div { class: "flex items-center gap-2",
            button {
                class: "w-10 h-10 p-0.5 rounded bg-black border border-neutral-600 flex items-center justify-center transition-colors hover:border-neutral-400",
                onclick: move |_| on_set.call(()),
                title: "Click a unit above, then click here to assign",
                if let Some(ref u) = unit {
                    img {
                        src: "/api/portraits/{u.id}.png",
                        alt: "{u.display_name}",
                        class: "w-full h-full object-contain",
                    }
                } else {
                    span { class: "text-neutral-500 text-lg", "?" }
                }
            }
            div { class: "flex flex-col",
                span { class: "text-xs text-neutral-300 truncate max-w-[120px]",
                    {unit.as_ref().map(|u| u.display_name.as_str()).unwrap_or("—")}
                }
                input {
                    r#type: "number",
                    value: "{count}",
                    min: "1",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f64>() {
                            on_count.call(v);
                        }
                    },
                    class: "w-16 px-1 py-0.5 bg-neutral-800 border border-neutral-700 rounded text-white text-xs focus:outline-none focus:border-blue-500",
                }
            }
        }
    }
}

#[component]
fn SimulationPage(plan: ConstructionPlan, on_back: EventHandler<()>) -> Element {
    rsx! {
        div {
            class: "flex flex-col h-screen bg-neutral-950 text-gray-200 font-sans overflow-hidden",
            header {
                class: "flex items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                button {
                    class: "px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    onclick: move |_| on_back.call(()),
                    "← Back to Builder"
                }
                h1 { class: "text-lg font-semibold text-white", "Simulation" }
            }
            div {
                class: "flex-1 flex items-center justify-center p-8",
                div { class: "text-center space-y-3",
                    h2 { class: "text-xl font-semibold text-white", "Simulation Page" }
                    p { class: "text-neutral-400", "This page will run the construction simulation." }
                    p { class: "text-neutral-500 text-sm", "Queued items: {plan.items.len()}" }
                }
            }
        }
    }
}
