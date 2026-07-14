use dioxus::prelude::*;
use faf_dioxus_ui::RGBColor;
use faf_sim::protocol::{ControlEvent, SimClientMessage, SimRuntimeStatus, SimServerMessage};
use faf_sim::sim::{EcoSnapshot, SimulationEvent};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

use crate::components::{ChartMetric, ChartSeries, ChartTab, UplotChart};
use crate::types::{ConstructionPlan, SimulationUiState};

/// Energy available for construction after paying maintenance.
fn energy_available(s: &EcoSnapshot) -> f64 {
    s.production_per_second_energy - s.maintenance_consumption_per_second_energy
}

/// Net energy change per second (income − maintenance − drain).
fn energy_net(s: &EcoSnapshot) -> f64 {
    energy_available(s) - s.energy_drain
}

/// FAF army-wide energy efficiency ratio used to scale mass income.
fn energy_efficiency(s: &EcoSnapshot) -> f64 {
    let requested = s.maintenance_consumption_per_second_energy + s.energy_drain;
    if requested <= 0.0 {
        1.0
    } else {
        (s.production_per_second_energy / requested).min(1.0)
    }
}

/// Mass income after applying FAF energy-stall scaling.
fn scaled_mass_income(s: &EcoSnapshot) -> f64 {
    if s.energy_storage < s.maintenance_consumption_per_second_energy {
        s.production_per_second_mass * energy_efficiency(s)
    } else {
        s.production_per_second_mass
    }
}

/// Net mass change per second (scaled income − drain).
fn mass_net(s: &EcoSnapshot) -> f64 {
    scaled_mass_income(s) - s.mass_drain
}

/// Constant 1.0 reference line for efficiency charts.
fn one(_s: &EcoSnapshot) -> f64 {
    1.0
}

const SIMULATION_DT_SECONDS: u32 = 1;
const MAX_SIMULATION_TIME_SECONDS: u32 = 3600;
const SIMULATION_TICK_INTERVAL_MS: u64 = 50;

/// Owns a WebSocket and the closures attached to it.
///
/// Storing the closures alongside the socket lets them be dropped together with
/// the socket, preventing the use-after-free that occurs when `closure.forget()`
/// is used and the component later unmounts.
struct WsHandle {
    ws: WebSocket,
    _onopen: Closure<dyn FnMut(Event)>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onerror: Closure<dyn FnMut(ErrorEvent)>,
    _onclose: Closure<dyn FnMut(CloseEvent)>,
}

impl Drop for WsHandle {
    fn drop(&mut self) {
        // Detach the closures before dropping them so the browser cannot invoke
        // a Rust closure that is being destroyed.
        let _ = self.ws.set_onopen(None);
        let _ = self.ws.set_onmessage(None);
        let _ = self.ws.set_onerror(None);
        let _ = self.ws.set_onclose(None);
        let _ = self.ws.close();
    }
}

#[component]
pub fn SimulationPanel(
    plan: Signal<ConstructionPlan>,
    mut state: Signal<SimulationUiState>,
) -> Element {
    let queue = use_memo(move || plan.read().to_build_queue());
    let snapshots = use_signal(Vec::<EcoSnapshot>::new);
    let mut ws = use_signal(|| None::<WsHandle>);
    let simulation_id = use_signal(|| None::<faf_sim::protocol::SimulationId>);
    let mut previous_queue = use_signal(|| queue.read().clone());

    // Release resources when the simulation is not running and auto-restart when
    // the plan changes while a simulation is visible.
    use_effect(move || {
        let queue = queue.read().clone();
        let current_state = *state.read();

        if current_state == SimulationUiState::NotStartYet {
            ws.set(None);
        }

        // Always keep previous_queue in sync with the current plan so that a
        // button-induced transition into Running never looks like a plan change.
        let prev = previous_queue.read().clone();
        let plan_changed = queue != prev;
        if plan_changed {
            previous_queue.set(queue.clone());
        }

        if current_state != SimulationUiState::NotStartYet && plan_changed {
            start_run(&queue, snapshots, ws, state, simulation_id);
        }
    });

    let current_state = *state.read();
    let snaps = snapshots.read();
    let current_time = snaps.last().map_or(0.0, |s| s.time);
    let is_finished = current_state == SimulationUiState::Finished;

    let start_enabled = current_state == SimulationUiState::NotStartYet;
    let pause_enabled =
        current_state == SimulationUiState::Running || current_state == SimulationUiState::Paused;
    let stop_enabled =
        current_state == SimulationUiState::Running || current_state == SimulationUiState::Paused;
    let reset_enabled = current_state == SimulationUiState::Running
        || current_state == SimulationUiState::Paused
        || current_state == SimulationUiState::Finished;
    let pause_label = if current_state == SimulationUiState::Paused {
        "Resume"
    } else {
        "Pause"
    };

    rsx! {
        div { class: "flex-1 flex flex-col min-h-0",
            div { class: "flex items-center justify-center gap-2 mb-3 shrink-0",
                ControlButton {
                    label: "Start",
                    enabled: start_enabled,
                    onclick: move |_| {
                        previous_queue.set(queue.read().clone());
                        start_run(&queue.read().clone(), snapshots, ws, state, simulation_id);
                    },
                }
                ControlButton {
                    label: pause_label.to_string(),
                    enabled: pause_enabled,
                    onclick: move |_| {
                        match *state.read() {
                            SimulationUiState::Paused => send_resume(ws, simulation_id),
                            _ => send_pause(ws, simulation_id),
                        }
                    },
                }
                ControlButton {
                    label: "Stop",
                    enabled: stop_enabled,
                    onclick: move |_| {
                        send_stop(ws, simulation_id);
                    },
                }
                ControlButton {
                    label: "Reset",
                    enabled: reset_enabled,
                    onclick: move |_| {
                        reset_run(snapshots, ws, state, simulation_id);
                    },
                }
            }
            if current_state == SimulationUiState::NotStartYet {
                div { class: "flex-1 flex items-center justify-center",
                    p { class: "text-sm text-neutral-500 text-center",
                        "Click \"Start\" to run the simulation."
                    }
                }
            } else if snaps.len() < 2 && is_finished {
                div { class: "flex-1 flex items-center justify-center",
                    p { class: "text-sm text-neutral-500 text-center",
                        "No simulation data."
                        br {}
                        "Make sure builders have a build rate and targets have mass/energy costs."
                    }
                }
            } else {
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
                                label: "Energy budget".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Income",
                                        RGBColor(34, 197, 94),
                                        ChartMetric::new(|s| s.production_per_second_energy),
                                    ),
                                    ChartSeries::new(
                                        "Maintenance",
                                        RGBColor(234, 179, 8),
                                        ChartMetric::new(|s| s.maintenance_consumption_per_second_energy),
                                    ),
                                    ChartSeries::new(
                                        "Available",
                                        RGBColor(59, 130, 246),
                                        ChartMetric::new(energy_available),
                                    ),
                                    ChartSeries::new(
                                        "Drain",
                                        RGBColor(239, 68, 68),
                                        ChartMetric::new(|s| s.energy_drain),
                                    ),
                                    ChartSeries::new(
                                        "Net",
                                        RGBColor(168, 85, 247),
                                        ChartMetric::new(energy_net),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Mass budget".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Gross income",
                                        RGBColor(156, 163, 175),
                                        ChartMetric::new(|s| s.production_per_second_mass),
                                    ),
                                    ChartSeries::new(
                                        "Scaled income",
                                        RGBColor(59, 130, 246),
                                        ChartMetric::new(scaled_mass_income),
                                    ),
                                    ChartSeries::new(
                                        "Drain",
                                        RGBColor(239, 68, 68),
                                        ChartMetric::new(|s| s.mass_drain),
                                    ),
                                    ChartSeries::new(
                                        "Net",
                                        RGBColor(34, 197, 94),
                                        ChartMetric::new(mass_net),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Efficiency".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Energy efficiency",
                                        RGBColor(59, 130, 246),
                                        ChartMetric::new(energy_efficiency),
                                    ),
                                    ChartSeries::new(
                                        "100%",
                                        RGBColor(156, 163, 175),
                                        ChartMetric::new(one),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Mass storage".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Current",
                                        RGBColor(99, 102, 241),
                                        ChartMetric::new(|s| s.mass_storage),
                                    ),
                                    ChartSeries::new(
                                        "Cap",
                                        RGBColor(168, 85, 247),
                                        ChartMetric::new(|s| s.mass_storage_cap),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Energy storage".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Current",
                                        RGBColor(14, 165, 233),
                                        ChartMetric::new(|s| s.energy_storage),
                                    ),
                                    ChartSeries::new(
                                        "Cap",
                                        RGBColor(236, 72, 153),
                                        ChartMetric::new(|s| s.energy_storage_cap),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Mass spent".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Total mass spent",
                                        RGBColor(34, 197, 94),
                                        ChartMetric::new(|s| s.total_mass_spent),
                                    ),
                                ],
                            },
                            ChartTab {
                                label: "Energy spent".to_string(),
                                series: vec![
                                    ChartSeries::new(
                                        "Total energy spent",
                                        RGBColor(249, 115, 22),
                                        ChartMetric::new(|s| s.total_energy_spent),
                                    ),
                                ],
                            },
                        ],
                    }
                }
            }
        }
    }
}

#[component]
fn ControlButton(label: String, enabled: bool, onclick: EventHandler<()>) -> Element {
    let base = "px-4 py-1.5 text-sm rounded transition-colors";
    let active_class = if enabled {
        "bg-blue-700 hover:bg-blue-600 text-white"
    } else {
        "bg-neutral-800 text-neutral-500 cursor-not-allowed"
    };

    rsx! {
        button {
            class: "{base} {active_class}",
            disabled: !enabled,
            onclick: move |_| {
                if enabled {
                    onclick.call(());
                }
            },
            "{label}"
        }
    }
}

fn reset_run(
    mut snapshots: Signal<Vec<EcoSnapshot>>,
    mut ws: Signal<Option<WsHandle>>,
    mut state: Signal<SimulationUiState>,
    mut simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    ws.set(None);
    snapshots.set(vec![]);
    simulation_id.set(None);
    state.set(SimulationUiState::NotStartYet);
}

fn start_run(
    queue: &faf_sim::sim::BuildQueue,
    mut snapshots: Signal<Vec<EcoSnapshot>>,
    mut ws: Signal<Option<WsHandle>>,
    mut state: Signal<SimulationUiState>,
    mut simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    // Drop the old handle first. Its Drop impl detaches closures and closes the
    // socket so stale callbacks cannot fire after this point.
    ws.set(None);
    snapshots.set(vec![]);
    state.set(SimulationUiState::Running);
    simulation_id.set(None);

    let new_ws = match WebSocket::new("/ws/simulate") {
        Ok(ws) => ws,
        Err(e) => {
            web_sys::console::error_1(&format!("failed to open websocket: {e:?}").into());
            state.set(SimulationUiState::NotStartYet);
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

    let onmessage = Closure::wrap(Box::new({
        let mut snapshots_signal = snapshots;
        let mut state_signal = state;
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
                            state_signal.set(SimulationUiState::NotStartYet);
                        }
                    }
                }
                Ok(SimServerMessage::Error { message }) => {
                    web_sys::console::error_1(&format!("simulation error: {message}").into());
                    state_signal.set(SimulationUiState::NotStartYet);
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

    let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
        web_sys::console::error_1(&format!("websocket error: {event:?}").into());
    }) as Box<dyn FnMut(ErrorEvent)>);
    new_ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

    let onclose = Closure::wrap(Box::new({
        let mut state_signal = state;
        move |_event: CloseEvent| {
            if *state_signal.read() != SimulationUiState::Finished {
                state_signal.set(SimulationUiState::NotStartYet);
            }
        }
    }) as Box<dyn FnMut(CloseEvent)>);
    new_ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

    ws.set(Some(WsHandle {
        ws: new_ws,
        _onopen: onopen,
        _onmessage: onmessage,
        _onerror: onerror,
        _onclose: onclose,
    }));
}

fn send_pause(
    ws: Signal<Option<WsHandle>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Pause { simulation_id: *id });
    }
}

fn send_resume(
    ws: Signal<Option<WsHandle>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Resume { simulation_id: *id });
    }
}

fn send_stop(
    ws: Signal<Option<WsHandle>>,
    simulation_id: Signal<Option<faf_sim::protocol::SimulationId>>,
) {
    if let Some(id) = simulation_id.read().as_ref() {
        send_message(ws, SimClientMessage::Stop { simulation_id: *id });
    }
}

fn send_message(ws: Signal<Option<WsHandle>>, msg: SimClientMessage) {
    if let Some(handle) = ws.read().as_ref() {
        match serde_json::to_string(&msg) {
            Ok(text) => {
                if let Err(e) = handle.ws.send_with_str(&text) {
                    web_sys::console::error_1(&format!("failed to send message: {e:?}").into());
                }
            }
            Err(e) => {
                web_sys::console::error_1(&format!("failed to serialize message: {e}").into());
            }
        }
    }
}
