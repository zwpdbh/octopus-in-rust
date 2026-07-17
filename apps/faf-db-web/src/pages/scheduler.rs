use dioxus::prelude::*;
use gloo_net::http::Request;

use crate::components::{AppHeader, BlueprintGraph, UnitDetail};
use crate::route::Route;
use crate::types::{BlueprintGraphResponse, UnitSummary};

#[component]
pub fn Scheduler() -> Element {
    let graph = use_resource(|| async move {
        Request::get("/api/blueprint-graph")
            .send()
            .await
            .ok()?
            .json::<BlueprintGraphResponse>()
            .await
            .ok()
    });
    let mut selected = use_signal(|| None::<UnitSummary>);

    rsx! {
        div { class: "flex flex-col h-screen bg-neutral-950 text-neutral-100",
            AppHeader { active: Route::Scheduler {} }

            main { class: "flex-1 overflow-hidden p-6 flex flex-col",
                h2 { class: "text-xl font-semibold mb-4 flex-shrink-0", "Scheduler — Blueprint Dependency Graph" }

                match graph.read().as_ref() {
                    Some(Some(response)) => rsx! {
                        div { class: "flex gap-4 flex-1 min-h-0",
                            div { class: "flex-1 min-w-0 flex flex-col border border-neutral-800 rounded bg-neutral-900 p-4 overflow-hidden",
                                GraphLegend {}
                                div { class: "flex-1 min-h-0 mt-3",
                                    BlueprintGraph {
                                        graph: response.graph.clone(),
                                        summaries: response.summaries.clone(),
                                        on_node_click: move |summary: UnitSummary| {
                                            selected.set(Some(summary));
                                        },
                                    }
                                }
                            }
                            div { class: "w-96 flex-shrink-0 border border-neutral-800 rounded bg-neutral-900 p-4 overflow-auto",
                                UnitDetail { selected }
                            }
                        }
                    },
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
        div { class: "flex gap-6 text-sm text-neutral-300",
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
