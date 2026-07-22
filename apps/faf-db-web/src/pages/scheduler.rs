use dioxus::prelude::*;
use dioxus_router::use_navigator;
use gloo_net::http::Request;

use crate::components::{
    AppHeader, GraphPopup, ScheduleFormState, ScheduleRequestPanel, ScheduleResultPanel,
};
use crate::route::Route;
use crate::state::save_plan_to_storage;
use crate::types::{
    BlueprintGraphResponse, ScheduleApiError, ScheduleUiState, ScheduleWithReasoning, UnitSummary,
};

#[component]
pub fn Scheduler() -> Element {
    // Dependency map data: picker list, popup, and unit summaries.
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
    let mut show_map = use_signal(|| false);
    let mut selected_step = use_signal(|| None::<usize>);

    let navigator = use_navigator();

    let on_compute = move |_| {
        let request = form.read().to_request();
        state.set(ScheduleUiState::Computing);
        selected_step.set(None);
        spawn(async move {
            let response = match Request::post("/api/schedule").json(&request) {
                Ok(builder) => builder.send().await,
                Err(e) => {
                    state.set(ScheduleUiState::Failed(format!("Invalid request: {e}")));
                    return;
                }
            };
            match response {
                Ok(resp) if resp.ok() => match resp.json::<ScheduleWithReasoning>().await {
                    Ok(payload) => {
                        state.set(ScheduleUiState::Success(payload.schedule, payload.reasoning));
                    }
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
            data.nodes
                .iter()
                .map(|node| (node.id.clone(), node.kind.clone()))
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();

    let reasoning = match &*state.read() {
        ScheduleUiState::Success(_, reasoning) => reasoning.clone(),
        _ => Vec::new(),
    };

    let on_send_to_simulate = move |_| {
        if let ScheduleUiState::Success(schedule, _) = &*state.read() {
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

                div { class: "flex gap-4 flex-1 min-h-0 min-w-0",
                    // Left: request form.
                    div { class: "w-[340px] flex-shrink-0 overflow-auto",
                        ScheduleRequestPanel { form, candidates, id_to_kind, computing, on_compute }
                    }

                    // Center: result with inline step details.
                    ScheduleResultPanel {
                        state,
                        form,
                        selected_step,
                        reasoning,
                        on_open_map: move |_| show_map.set(true),
                        on_send_to_simulate,
                    }
                }
            }

            if let Some(data) = graph_data {
                GraphPopup {
                    open: *show_map.read(),
                    data,
                    focus: None,
                    on_node_click: move |_summary: UnitSummary| {},
                    on_close: move |_| show_map.set(false),
                }
            }
        }
    }
}
