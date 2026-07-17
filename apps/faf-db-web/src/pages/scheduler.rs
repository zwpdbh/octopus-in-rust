use dioxus::prelude::*;
use dioxus_router::use_navigator;
use gloo_net::http::Request;

use crate::components::{
    AppHeader, GraphPopup, ResultTab, ScheduleFormState, ScheduleRequestPanel, ScheduleResultPanel,
    UnitDetail,
};
use crate::route::Route;
use crate::state::save_plan_to_storage;
use crate::types::{
    BlueprintGraphResponse, Schedule, ScheduleApiError, ScheduleUiState, UnitKind, UnitSummary,
};
use crate::utils::kind_node_id;

#[component]
pub fn Scheduler() -> Element {
    // Dependency map data: picker list, popup, and step-click summaries.
    let graph = use_resource(|| async move {
        Request::get("/api/blueprint-graph")
            .send()
            .await
            .ok()?
            .json::<BlueprintGraphResponse>()
            .await
            .ok()
    });

    let form = use_signal(ScheduleFormState::default);
    let mut state = use_signal(|| ScheduleUiState::Idle);
    let tab = use_signal(|| ResultTab::Timeline);
    let mut show_map = use_signal(|| false);
    let mut selected = use_signal(|| None::<UnitSummary>);

    let navigator = use_navigator();

    let on_compute = move |_| {
        let request = form.read().to_request();
        state.set(ScheduleUiState::Computing);
        spawn(async move {
            let response = match Request::post("/api/schedule").json(&request) {
                Ok(builder) => builder.send().await,
                Err(e) => {
                    state.set(ScheduleUiState::Failed(format!("Invalid request: {e}")));
                    return;
                }
            };
            match response {
                Ok(resp) if resp.ok() => match resp.json::<Schedule>().await {
                    Ok(schedule) => state.set(ScheduleUiState::Success(schedule)),
                    Err(e) => state.set(ScheduleUiState::Failed(format!("Invalid response: {e}"))),
                },
                Ok(resp) => {
                    let message = match resp.json::<ScheduleApiError>().await {
                        Ok(err) => err.error,
                        Err(_) => format!("HTTP {}", resp.status()),
                    };
                    state.set(ScheduleUiState::Failed(message));
                }
                Err(e) => state.set(ScheduleUiState::Failed(format!("Request failed: {e}"))),
            }
        });
    };

    let graph_data = graph.read().clone().flatten();

    // Units offered in the picker modal, and the lookup from a unit's
    // blueprint id back to its abstract kind (for building the request).
    let candidates = graph_data
        .as_ref()
        .map(|data| {
            let mut candidates: Vec<UnitSummary> = data.summaries.values().cloned().collect();
            candidates.sort_by(|a, b| a.display_name.cmp(&b.display_name));
            candidates
        })
        .unwrap_or_default();
    let id_to_kind = graph_data
        .as_ref()
        .map(|data| {
            data.graph
                .graph
                .node_weights()
                .filter_map(|node| {
                    data.summaries
                        .get(&kind_node_id(&node.kind))
                        .map(|summary| (summary.id.clone(), node.kind.clone()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();

    let summaries_for_click = graph_data
        .as_ref()
        .map(|data| data.summaries.clone())
        .unwrap_or_default();
    let on_step_click = move |kind: UnitKind| {
        if let Some(summary) = summaries_for_click.get(&kind_node_id(&kind)) {
            selected.set(Some(summary.clone()));
        }
    };

    let on_send_to_simulate = move |_| {
        if let ScheduleUiState::Success(schedule) = &*state.read() {
            save_plan_to_storage(&schedule.plan);
            navigator.push(Route::SimulateBuild {});
        }
    };

    let computing = matches!(*state.read(), ScheduleUiState::Computing);

    rsx! {
        div { class: "flex flex-col h-screen bg-neutral-950 text-neutral-100",
            AppHeader { active: Route::Scheduler {} }

            main { class: "flex-1 overflow-hidden p-6 flex flex-col",
                h2 { class: "text-xl font-semibold mb-4 flex-shrink-0", "Scheduler" }

                div { class: "flex gap-4 flex-1 min-h-0",
                    // Left: request form.
                    div { class: "w-[340px] flex-shrink-0 overflow-auto",
                        ScheduleRequestPanel { form, candidates, id_to_kind, computing, on_compute }
                    }

                    // Center: result.
                    ScheduleResultPanel {
                        state,
                        form,
                        tab,
                        on_step_click,
                        on_open_map: move |_| show_map.set(true),
                        on_send_to_simulate,
                    }

                    // Right: details of the clicked unit/step/graph node.
                    div { class: "w-96 flex-shrink-0 border border-neutral-800 rounded bg-neutral-900 p-4 overflow-auto",
                        UnitDetail { selected }
                    }
                }
            }

            if let Some(data) = graph_data {
                GraphPopup {
                    open: *show_map.read(),
                    data,
                    on_node_click: move |summary: UnitSummary| selected.set(Some(summary)),
                    on_close: move |_| show_map.set(false),
                }
            }
        }
    }
}
