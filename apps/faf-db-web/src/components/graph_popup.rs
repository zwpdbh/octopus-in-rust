use dioxus::prelude::*;
use faf_sim_shared::plan::UnitSummary;

use crate::components::BlueprintGraph;
use crate::types::BlueprintGraphResponse;

/// Modal popup showing the blueprint dependency graph as a consultable map.
/// Opened from the result panel's "Map" button; closed via backdrop or ✕.
#[component]
pub fn GraphPopup(
    open: bool,
    data: BlueprintGraphResponse,
    on_node_click: EventHandler<UnitSummary>,
    on_close: EventHandler<()>,
) -> Element {
    if !open {
        return rsx! {};
    }

    rsx! {
        div { class: "fixed inset-0 z-50 flex items-center justify-center bg-black/70",
            onclick: move |_| on_close.call(()),
            div { class: "w-[92vw] h-[85vh] bg-neutral-900 rounded-lg border border-neutral-700 shadow-2xl overflow-hidden flex flex-col",
                onclick: move |e| e.stop_propagation(),
                div { class: "flex items-center justify-between px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
                    div { class: "flex items-center gap-6",
                        h3 { class: "text-sm font-semibold text-white", "Blueprint Dependency Graph" }
                        div { class: "flex gap-4 text-xs text-neutral-300",
                            span { class: "flex items-center gap-1.5",
                                LegendArrow { color: "#38bdf8", dashed: false }
                                "built by"
                            }
                            span { class: "flex items-center gap-1.5",
                                LegendArrow { color: "#94a3b8", dashed: true }
                                "requires"
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
                div { class: "flex-1 min-h-0",
                    BlueprintGraph {
                        graph: data.graph,
                        summaries: data.summaries,
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
