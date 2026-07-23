use dioxus::prelude::*;
use dioxus_router::use_navigator;
use gloo_net::http::Request;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::components::{
    AppHeader, GraphPopup, ScheduleFormState, ScheduleRequestPanel, ScheduleResultPanel,
};
use crate::route::Route;
use crate::state::save_plan_to_storage;
use crate::types::{
    BlueprintGraphResponse, ScheduleUiState, ScheduleWsClientMessage, ScheduleWsServerMessage,
    UnitSummary,
};

/// Handle to an in-flight scheduling WebSocket, allowing the user to cancel it.
#[derive(Clone)]
pub struct ScheduleController {
    cancel: std::rc::Rc<dyn Fn()>,
}

impl PartialEq for ScheduleController {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.cancel, &other.cancel)
    }
}

impl ScheduleController {
    pub fn cancel(&self) {
        (self.cancel)();
    }
}

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
    let mut controller = use_signal(|| None::<ScheduleController>);
    let mut show_map = use_signal(|| false);
    let mut selected_step = use_signal(|| None::<usize>);

    let navigator = use_navigator();

    let on_compute = move |_| {
        let request = form.read().to_request();
        state.set(ScheduleUiState::Streaming {
            steps: Vec::new(),
            reasoning: Vec::new(),
        });
        selected_step.set(None);

        let ws = match web_sys::WebSocket::new("/ws/schedule") {
            Ok(ws) => ws,
            Err(e) => {
                state.set(ScheduleUiState::Failed(format!(
                    "Failed to open WebSocket: {e:?}"
                )));
                return;
            }
        };
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let cancel_ws = ws.clone();
        let cancel = std::rc::Rc::new(move || {
            if cancel_ws.ready_state() == web_sys::WebSocket::OPEN {
                let cancel_msg = ScheduleWsClientMessage::Cancel;
                let text = serde_json::to_string(&cancel_msg).unwrap_or_default();
                let _ = cancel_ws.send_with_str(&text);
                let _ = cancel_ws.close();
            }
        }) as std::rc::Rc<dyn Fn()>;
        controller.set(Some(ScheduleController {
            cancel: cancel.clone(),
        }));

        let mut state_for_message = state.clone();
        let mut state_for_close = state.clone();
        let mut state_for_error = state.clone();
        let mut controller_for_close = controller.clone();

        let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<ScheduleWsServerMessage>(&text) {
                    Ok(ScheduleWsServerMessage::Step {
                        step, reasoning, ..
                    }) => {
                        let mut steps = match state_for_message.read().clone() {
                            ScheduleUiState::Streaming { steps, .. } => steps,
                            _ => Vec::new(),
                        };
                        let mut reasoning_list = match state_for_message.read().clone() {
                            ScheduleUiState::Streaming { reasoning, .. } => reasoning,
                            _ => Vec::new(),
                        };
                        steps.push(step);
                        reasoning_list.push(reasoning);
                        state_for_message.set(ScheduleUiState::Streaming {
                            steps,
                            reasoning: reasoning_list,
                        });
                    }
                    Ok(ScheduleWsServerMessage::Done {
                        schedule,
                        reasoning,
                    }) => {
                        state_for_message.set(ScheduleUiState::Success(schedule, reasoning));
                        controller_for_close.set(None);
                    }
                    Ok(ScheduleWsServerMessage::Error { message }) => {
                        state_for_message.set(ScheduleUiState::Failed(message));
                        controller_for_close.set(None);
                    }
                    Err(e) => {
                        state_for_message.set(ScheduleUiState::Failed(format!(
                            "Invalid server message: {e}"
                        )));
                        controller_for_close.set(None);
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let onerror = Closure::wrap(Box::new(move |_e: web_sys::Event| {
            state_for_error.set(ScheduleUiState::Failed(
                "WebSocket error while scheduling".to_string(),
            ));
        }) as Box<dyn FnMut(_)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let onclose = Closure::wrap(Box::new(move |_e: web_sys::CloseEvent| {
            // If the socket closes and we are still streaming, treat it as a
            // cancellation/failure unless a final state has already been set.
            let still_streaming = matches!(
                state_for_close.read().clone(),
                ScheduleUiState::Streaming { .. }
            );
            if still_streaming {
                state_for_close.set(ScheduleUiState::Failed(
                    "Scheduling connection closed unexpectedly".to_string(),
                ));
            }
            controller_for_close.set(None);
        }) as Box<dyn FnMut(_)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        let ws_for_open = ws.clone();
        let onopen = Closure::wrap(Box::new(move |_e: web_sys::Event| {
            let start_msg = ScheduleWsClientMessage::Start {
                request: request.clone(),
            };
            let text = serde_json::to_string(&start_msg).unwrap_or_default();
            let _ = ws_for_open.send_with_str(&text);
        }) as Box<dyn FnMut(_)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();
    };

    let on_cancel = move |_| {
        let ctrl = controller.read().clone();
        if let Some(ctrl) = ctrl {
            ctrl.cancel();
            controller.set(None);
            if matches!(state.read().clone(), ScheduleUiState::Streaming { .. }) {
                state.set(ScheduleUiState::Failed("Scheduling cancelled".to_string()));
            }
        }
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
        ScheduleUiState::Streaming { reasoning, .. } => reasoning.clone(),
        _ => Vec::new(),
    };

    let on_send_to_simulate = move |_| {
        if let ScheduleUiState::Success(schedule, _) = &*state.read() {
            save_plan_to_storage(&schedule.plan);
            navigator.push(Route::SimulateBuild {});
        }
    };

    let computing = matches!(*state.read(), ScheduleUiState::Streaming { .. });

    rsx! {
        div { class: "flex flex-col h-screen bg-neutral-950 text-neutral-100",
            AppHeader { active: Route::Scheduler {} }

            main { class: "flex-1 overflow-hidden p-6 flex flex-col",
                h2 { class: "text-xl font-semibold mb-4 flex-shrink-0", "Scheduler" }

                div { class: "flex gap-4 flex-1 min-h-0 min-w-0",
                    // Left: request form.
                    div { class: "w-[340px] flex-shrink-0 overflow-auto",
                        ScheduleRequestPanel {
                            form,
                            candidates,
                            id_to_kind,
                            computing,
                            on_compute,
                            on_cancel,
                        }
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
