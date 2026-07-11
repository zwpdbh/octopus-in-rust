use dioxus::prelude::*;
use gloo_net::http::Request;

use crate::components::Stat;
use crate::types::{UnitDetailData, UnitSummary};
use crate::utils::{faction_color, faction_glow_class};

#[component]
pub fn UnitDetail(selected: Signal<Option<UnitSummary>>) -> Element {
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
