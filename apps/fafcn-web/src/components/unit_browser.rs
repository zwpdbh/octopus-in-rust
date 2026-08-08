use dioxus::prelude::*;
use gloo_net::http::Request;

use crate::components::{UnitSelector, UnitSummary};
use crate::utils::{faction_color, faction_glow_class};

/// A browsable unit database for the home page.
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
    let mut selected = use_signal(|| None::<UnitSummary>);

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

    rsx! {
        div { class: "flex flex-col h-full bg-neutral-950 text-gray-200 overflow-hidden font-sans select-none",
            div { class: "flex flex-1 min-h-0 overflow-hidden",
                div { class: "flex-1 overflow-hidden",
                    UnitSelector {
                        units: unit_list,
                        on_select: move |unit: UnitSummary| {
                            selected.with_mut(|s| {
                                if s.as_ref().map(|sel| sel.id == unit.id).unwrap_or(false) {
                                    *s = None;
                                } else {
                                    *s = Some(unit);
                                }
                            });
                        },
                    }
                }
                div { class: "w-96 shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-y-auto p-4",
                    UnitDetailPanel { unit: selected }
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
