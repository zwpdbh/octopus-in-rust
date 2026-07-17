use dioxus::prelude::*;
use faf_sim_shared::plan::UnitSummary;

use crate::components::BlueprintGraph;
use crate::types::BlueprintGraphResponse;
use crate::utils::FACTION_ORDER;

/// Modal popup showing the concrete blueprint dependency graph as a
/// consultable map, one faction at a time. When `focus` is given, the popup
/// opens on that unit's faction tab and highlights the unit.
#[component]
pub fn GraphPopup(
    open: bool,
    data: BlueprintGraphResponse,
    /// Optional unit to focus: presets the faction tab and highlights the node.
    focus: Option<UnitSummary>,
    on_node_click: EventHandler<UnitSummary>,
    on_close: EventHandler<()>,
) -> Element {
    let initial = focus
        .as_ref()
        .map(|u| normalize_faction(&u.faction))
        .unwrap_or_else(|| "UEF".to_string());
    let mut faction = use_signal(|| initial.clone());

    // Reset the tab whenever the popup is (re-)opened with a new focus.
    use_effect(use_reactive!(|open, initial| {
        if open {
            faction.set(initial);
        }
    }));

    if !open {
        return rsx! {};
    }

    let active = faction.read().clone();
    let highlight = focus.as_ref().map(|u| u.id.clone());

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/70",
            onclick: move |_| on_close.call(()),
            div { class: "w-[92vw] h-[85vh] bg-neutral-900 rounded-lg border border-neutral-700 shadow-2xl overflow-hidden flex flex-col",
                onclick: move |e| e.stop_propagation(),
                div { class: "flex items-center justify-between px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                    div { class: "flex items-center gap-4",
                        h3 { class: "text-sm font-semibold text-white", "Blueprint Dependency Graph" }
                        div { class: "flex gap-3 text-xs text-neutral-300",
                            span { class: "flex items-center gap-1.5",
                                LegendArrow { color: "#38bdf8", dashed: false }
                                "built by"
                            }
                            span { class: "flex items-center gap-1.5",
                                LegendArrow { color: "#fbbf24", dashed: true }
                                "upgrades to"
                            }
                        }
                    }
                    button {
                        class: "px-2 py-1 text-lg leading-none text-neutral-400 hover:text-white transition-colors",
                        onclick: move |_| on_close.call(()),
                        "×"
                    }
                }
                div { class: "flex items-center gap-1 px-4 py-2 border-b border-neutral-800 shrink-0",
                    for f in FACTION_ORDER.iter().copied() {
                        {
                            let is_active = active == f;
                            rsx! {
                                button {
                                    class: if is_active {
                                        "px-3 py-1 text-xs font-semibold rounded bg-blue-700 text-white transition-colors"
                                    } else {
                                        "px-3 py-1 text-xs font-semibold rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
                                    },
                                    onclick: move |_| faction.set(f.to_string()),
                                    "{f}"
                                }
                            }
                        }
                    }
                }
                div { class: "flex-1 min-h-0",
                    BlueprintGraph {
                        nodes: data.nodes.clone(),
                        edges: data.edges.clone(),
                        summaries: data.summaries.clone(),
                        faction: active.clone(),
                        highlight: highlight.clone(),
                        on_node_click,
                    }
                }
            }
        }
    }
}

#[component]
fn LegendArrow(color: String, dashed: bool) -> Element {
    let dash_array = if dashed { "4,4" } else { "none" };
    rsx! {
        svg {
            width: "28",
            height: "12",
            view_box: "0 0 28 12",
            class: "inline-block",
            line {
                x1: "0",
                y1: "6",
                x2: "21",
                y2: "6",
                stroke: "{color}",
                stroke_width: "2",
                stroke_dasharray: "{dash_array}",
            }
            polygon { points: "21 2, 28 6, 21 10", fill: "{color}" }
        }
    }
}

/// Unit summaries carry display factions like "UEF"; anything unexpected falls
/// back to UEF.
fn normalize_faction(faction: &str) -> String {
    if FACTION_ORDER.contains(&faction) {
        faction.to_string()
    } else {
        "UEF".to_string()
    }
}
