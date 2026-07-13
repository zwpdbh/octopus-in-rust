use dioxus::prelude::*;
use faf_dioxus_ui::RGBColor;
use faf_sim::protocol::{ControlEvent, SimClientMessage, SimRuntimeStatus, SimServerMessage};
use faf_sim::sim::{EcoSnapshot, SimulationEvent};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

use crate::components::{ChartMetric, ChartTab, UplotChart};
use crate::types::{ConstructionPlan, SimulationUiState};

const SIMULATION_DT_SECONDS: u32 = 1;
const MAX_SIMULATION_TIME_SECONDS: u32 = 3600;
const SIMULATION_TICK_INTERVAL_MS: u64 = 50;

/// Commands the parent control bar can issue to the simulation panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationCommand {
    Start,
    Pause,
    Resume,
    Stop,
    Reset,
}

#[component]
pub fn SimulationPanel(
    plan: ConstructionPlan,
    mut state: Signal<SimulationUiState>,
    mut command: Signal<Option<SimulationCommand>>,
) -> Element {
    let queue = use_memo(move || plan.to_build_queue());
    let snapshots = use_signal(Vec::<EcoSnapshot>::new);
    let ws = use_signal(|| None::<WebSocket>);
    let simulation_id = use_signal(|| None::<faf_sim::protocol::SimulationId>);
    let mut previous_queue = use_signal(|| queue.read().clone());

    // React to commands from the parent and to plan changes while a simulation
    // is visible.
    use_effect(move || {
        let queue = queue.read().clone();
        let current_state = *state.read();
        let cmd = command.read().clone();

        if let Some(cmd) = cmd {
            command.set(None);
            match cmd {
                SimulationCommand::Start | SimulationCommand::Reset => {
                    previous_queue.set(queue.clone());
                    start_run(&queue, snapshots, ws, state, simulation_id);
                }
                SimulationCommand::Pause => send_pause(ws, simulation_id),
                SimulationCommand::Resume => send_resume(ws, simulation_id),
                SimulationCommand::Stop => send_stop(ws, simulation_id),
            }
            return;
        }

        if current_state != SimulationUiState::Idle {
            let prev = previous_queue.read().clone();
            if queue != prev {
                previous_queue.set(queue.clone());
                start_run(&queue, snapshots, ws, state, simulation_id);
            }
        }
    });

    let snaps = snapshots.read();
    let current_time = snaps.last().map_or(0.0, |s| s.time);
    let is_finished = *state.read() == SimulationUiState::Finished;

    if snaps.len() < 2 && is_finished {
        return rsx! {
            div { class: "flex-1 flex items-center justify-center",
                p { class: "text-sm text-neutral-500 text-center",
                    "No simulation data."
                    br {}
                    "Make sure builders have a build rate and targets have mass/energy costs."
                }
            }
        };
    }

    rsx! {
        div { class: "flex-1 flex flex-col min-h-0",
            div { class: "flex items-center gap-2 mb-3 shrink-0",
                span { class: "text-sm text-neutral-400 tabular-nums ml-auto",
                    if is_finished {
                        "{current_time:.1}s / {current_time:.1}s"
                    } else {
                        "{current_time:.1}s / ..."
                    }
                }
            }
            UplotChart {
                data: snapshots,
                x_extractor: ChartMetric::new(|s: &EcoSnapshot| s.time),
                tabs: vec![
                    ChartTab {
                        label: "Mass income".to_string(),
                        color: RGBColor(59, 130, 246),
                        y_extractor: ChartMetric::new(|s| s.mass_income),
                    },
                    ChartTab {
                        label: "Energy income".to_string(),
                        color: RGBColor(234, 179, 8),
                        y_extractor: ChartMetric::new(|s| s.energy_income),
                    },
                    ChartTab {
                        label: "Total mass spent".to_string(),
                        color: RGBColor(34, 197, 94),
                        y_extractor: ChartMetric::new(|s| s.total_mass_spent),
                    },
                    ChartTab {
                        label: "Total energy spent".to_string(),
                        color: RGBColor(249, 115, 22),
                        y_extractor: ChartMetric::new(|s| s.total_energy_spent),
                    },
                ],
            }
        }
    }
}

fn start_run(
    queue: &faf_sim::sim::BuildQueue,
    mut snapshots: Signal<Vec<EcoSnapshot>>,
    mut ws: Signal<Option<WebSocket>>,
    mut state: Signal<SimulationUiState>,
    mut simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(old_ws) = ws.write().take() {
        let _ = old_ws.close();
    }
    snapshots.set(vec![]);
    state.set(SimulationUiState::Running);
    simulation_id.set(None);

    let new_ws = match WebSocket::new("/ws/simulate") {
        Ok(ws) => ws,
        Err(e) => {
            web_sys::console::error_1(&format!("failed to open websocket: {e:?}").into());
            state.set(SimulationUiState::Idle);
            return;
        }
    };

    let onopen = Closure::wrap(Box::new({
        let ws = new_ws.clone();
        let queue = queue.clone();
        move |_event: Event| {
            let start = SimClientMessage::Start {
                queue: queue.clone(),
                dt_seconds: SIMULATION_DT_SECONDS,
                max_time_seconds: Some(MAX_SIMULATION_TIME_SECONDS),
                mode: faf_sim::protocol::SimulationMode::Passive {
                    tick_interval_ms: SIMULATION_TICK_INTERVAL_MS,
                },
            };
            match serde_json::to_string(&start) {
                Ok(text) => {
                    if let Err(e) = ws.send_with_str(&text) {
                        web_sys::console::error_1(
                            &format!("failed to send start message: {e:?}").into(),
                        );
                    }
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("failed to serialize start message: {e}").into(),
                    );
                }
            }
        }
    }) as Box<dyn FnMut(Event)>);
    new_ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage = Closure::wrap(Box::new({
        let mut snapshots_signal = snapshots;
        let mut state_signal = state;
        let mut ws_signal = ws;
        let mut simulation_id_signal = simulation_id;
        move |event: MessageEvent| {
            let text = event.data().as_string().unwrap_or_default();
            match serde_json::from_str::<SimServerMessage>(&text) {
                Ok(SimServerMessage::Started { simulation_id: id }) => {
                    simulation_id_signal.set(Some(id));
                }
                Ok(SimServerMessage::Event(SimulationEvent::Ticked(snapshot))) => {
                    snapshots_signal.with_mut(|v| v.push(snapshot));
                }
                Ok(SimServerMessage::Event(SimulationEvent::Finished)) => {
                    state_signal.set(SimulationUiState::Finished);
                }
                Ok(SimServerMessage::Event(_)) => {}
                Ok(SimServerMessage::ControlEvent(ControlEvent::StateChanged { to, .. })) => {
                    match to {
                        SimRuntimeStatus::Running => {
                            state_signal.set(SimulationUiState::Running);
                        }
                        SimRuntimeStatus::Paused => {
                            state_signal.set(SimulationUiState::Paused);
                        }
                        SimRuntimeStatus::Stopped => {
                            state_signal.set(SimulationUiState::Idle);
                        }
                    }
                }
                Ok(SimServerMessage::Error { message }) => {
                    web_sys::console::error_1(&format!("simulation error: {message}").into());
                    state_signal.set(SimulationUiState::Idle);
                    ws_signal.set(None);
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("failed to parse simulation message: {e}").into(),
                    );
                }
            }
        }
    }) as Box<dyn FnMut(MessageEvent)>);
    new_ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
        web_sys::console::error_1(&format!("websocket error: {event:?}").into());
    }) as Box<dyn FnMut(ErrorEvent)>);
    new_ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    let onclose = Closure::wrap(Box::new({
        let mut state_signal = state;
        let mut ws_signal = ws;
        move |_event: CloseEvent| {
            // If the simulation finished naturally we want to keep the chart
            // visible; otherwise treat an unexpected close as a stop.
            if *state_signal.read() != SimulationUiState::Finished {
                state_signal.set(SimulationUiState::Idle);
            }
            ws_signal.set(None);
        }
    }) as Box<dyn FnMut(CloseEvent)>);
    new_ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    onclose.forget();

    ws.set(Some(new_ws));
}

fn send_pause(
    ws: Signal<Option<WebSocket>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Pause { simulation_id: *id });
    }
}

fn send_resume(
    ws: Signal<Option<WebSocket>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Resume { simulation_id: *id });
    }
}

fn send_stop(
    ws: Signal<Option<WebSocket>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Stop { simulation_id: *id });
    }
}

fn send_message(ws: Signal<Option<WebSocket>>, msg: SimClientMessage) {
    if let Some(ws) = ws.read().as_ref() {
        match serde_json::to_string(&msg) {
            Ok(text) => {
                if let Err(e) = ws.send_with_str(&text) {
                    web_sys::console::error_1(&format!("failed to send message: {e:?}").into());
                }
            }
            Err(e) => {
                web_sys::console::error_1(&format!("failed to serialize message: {e}").into());
            }
        }
    }
}
