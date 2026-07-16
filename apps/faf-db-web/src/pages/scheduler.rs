use dioxus::prelude::*;
use faf_dioxus_ui::{GraphData, GraphEdgeData, GraphInput, GraphNodeData, GraphOptions, GraphView};
use gloo_net::http::Request;
use petgraph::graph::DiGraph;

use crate::components::AppHeader;
use crate::route::Route;

#[component]
pub fn Scheduler() -> Element {
    let graph = use_resource(|| async move {
        Request::get("/api/blueprint-graph")
            .send()
            .await
            .ok()?
            .json::<GraphData>()
            .await
            .ok()
    });

    rsx! {
        div { class: "flex flex-col h-screen bg-neutral-950 text-neutral-100",
            AppHeader { active: Route::Scheduler {} }

            main { class: "flex-1 overflow-auto p-6",
                h2 { class: "text-xl font-semibold mb-4", "Scheduler — Blueprint Dependency Graph" }

                match graph.read().as_ref() {
                    Some(Some(data)) => {
                        let digraph: DiGraph<GraphNodeData, GraphEdgeData> = DiGraph::from(data);
                        rsx! {
                            div { class: "w-full overflow-auto border border-neutral-800 rounded bg-neutral-900 p-4",
                                GraphLegend {}
                                GraphView {
                                    graph: GraphInput(digraph),
                                    options: GraphOptions {
                                        min_width: 800,
                                        min_height: 600,
                                        ..Default::default()
                                    },
                                }
                            }
                        }
                    }
                    Some(None) => rsx! {
                        p { class: "text-red-400", "Failed to load blueprint graph." }
                    },
                    None => rsx! {
                        p { class: "text-neutral-400", "Loading blueprint graph..." }
                    },
                }
            }
        }
    }
}

#[component]
fn GraphLegend() -> Element {
    rsx! {
        div { class: "flex gap-6 mt-3 text-sm text-neutral-300",
            span { class: "flex items-center gap-2",
                span {
                    class: "inline-block w-8 border-t-2",
                    style: "border-color: #38bdf8;",
                }
                "built by"
            }
            span { class: "flex items-center gap-2",
                span {
                    class: "inline-block w-8 border-t-2 border-dashed",
                    style: "border-color: #94a3b8;",
                }
                "requires"
            }
            span { class: "flex items-center gap-2",
                span {
                    class: "inline-block w-8 border-t-2 border-dashed",
                    style: "border-color: #fbbf24;",
                }
                "upgrades to"
            }
        }
    }
}
