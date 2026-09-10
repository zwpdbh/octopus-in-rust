use dioxus::prelude::*;
use gloo_net::http::Request;

use crate::components::UnitSummary;
use crate::utils::{faction_color, faction_file_prefix, tech_level_short};
use crate::Route;

/// Fetch one unit summary by blueprint id.
async fn fetch_unit(id: &str) -> Result<UnitSummary, String> {
    let resp = Request::get(&crate::net::api_url(&format!("/api/units/{id}")))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status() == 404 {
        return Err(format!("unit {id:?} not found"));
    }
    resp.json::<UnitSummary>().await.map_err(|e| e.to_string())
}

/// One stat row in a detail section.
#[component]
fn StatRow(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "flex items-center justify-between py-1.5 border-b border-neutral-800/60 last:border-0",
            span { class: "text-xs text-neutral-400", "{label}" }
            span { class: "text-sm text-neutral-100 tabular-nums", "{value}" }
        }
    }
}

/// Unit detail page (`/units/:id`): portrait, identity, cost and economy
/// stats for a single unit. Reached by clicking a unit name in the
/// comparison sidebar.
#[component]
pub fn UnitDetail(id: String) -> Element {
    let unit = use_resource({
        let id = id.clone();
        move || {
            let id = id.clone();
            async move { fetch_unit(&id).await }
        }
    });

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans p-6",
            div { class: "max-w-3xl mx-auto",
                Link {
                    class: "text-xs text-blue-400 hover:underline",
                    to: Route::Units {},
                    "← back to unit browser"
                }
                match &*unit.read() {
                    None => rsx! { p { class: "text-neutral-400 mt-6", "Loading..." } },
                    Some(Err(e)) => rsx! { p { class: "text-red-400 mt-6", "{e}" } },
                    Some(Ok(u)) => {
                        let color = faction_color(&u.faction);
                        let strategic_src = u
                            .strategic_icon_name
                            .as_deref()
                            .map(|icon| {
                                format!(
                                    "/strategic/{}_{}.png",
                                    faction_file_prefix(&u.faction),
                                    icon
                                )
                            });
                        let eco_rows: Vec<(&'static str, f64)> = [
                            ("Mass generation /s", u.eco_effect.generate_mass_rate),
                            ("Energy generation /s", u.eco_effect.generate_energy_rate),
                            ("Maintenance energy drain /s", u.eco_effect.maintainance_energy_drain),
                            ("Mass storage bonus", u.eco_effect.increase_mass_storage_capacity),
                            ("Energy storage bonus", u.eco_effect.increase_energy_storage_capacity),
                            ("Build power", u.eco_effect.build_power),
                        ]
                        .into_iter()
                        .filter(|(_, v)| *v != 0.0)
                        .collect();
                        rsx! {
                            // Header: portrait + identity.
                            div { class: "flex items-start gap-5 mt-4 mb-6",
                                img {
                                    class: "w-24 h-24 object-contain rounded bg-black border border-neutral-800 p-1 shrink-0",
                                    src: crate::net::portrait_url(&u.id),
                                    alt: "{u.name}",
                                }
                                div { class: "min-w-0",
                                    h1 { class: "text-2xl font-bold", style: "color: {color};", "{u.name}" }
                                    p { class: "text-xs text-neutral-500 font-mono mt-0.5", "{u.id}" }
                                    div { class: "flex flex-wrap items-center gap-2 mt-2 text-xs",
                                        span { class: "px-2 py-0.5 rounded bg-neutral-800 text-neutral-300 uppercase tracking-wide", "{u.faction}" }
                                        span { class: "px-2 py-0.5 rounded bg-neutral-800 text-neutral-300", "{tech_level_short(u.tech_level)}" }
                                        if let Some(category) = &u.category {
                                            span { class: "px-2 py-0.5 rounded bg-neutral-800 text-neutral-300", "{category}" }
                                        }
                                        if let Some(kind) = &u.kind {
                                            span { class: "px-2 py-0.5 rounded bg-neutral-800 text-neutral-300", "{kind}" }
                                        }
                                    }
                                    if let (Some(src), Some(icon)) = (strategic_src, &u.strategic_icon_name) {
                                        div { class: "flex items-center gap-2 mt-3",
                                            img { class: "w-5 h-5 object-contain", src: "{src}", alt: "{icon}" }
                                            span { class: "text-xs text-neutral-500 font-mono", "{icon}" }
                                        }
                                    }
                                }
                            }

                            // Cost.
                            div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-4 mb-4",
                                h2 { class: "text-sm font-semibold text-white mb-2", "Cost" }
                                StatRow { label: "Mass", value: format!("{:.0}", u.cost.mass) }
                                StatRow { label: "Energy", value: format!("{:.0}", u.cost.energy) }
                                StatRow { label: "Build time", value: format!("{:.0}", u.cost.build_time) }
                            }

                            // Economy effect (non-zero entries only).
                            div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-4",
                                h2 { class: "text-sm font-semibold text-white mb-2", "Economy effect" }
                                if eco_rows.is_empty() {
                                    p { class: "text-xs text-neutral-500", "No economy effect." }
                                }
                                for (label, value) in eco_rows {
                                    StatRow { key: "{label}", label, value: format!("{value}") }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
