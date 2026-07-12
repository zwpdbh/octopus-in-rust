use dioxus::prelude::*;
use faf_dioxus_ui::RGBColor;
use faf_sim::protocol::{SimClientMessage, SimServerMessage};
use faf_sim::sim::{EcoSnapshot, SimulationEvent};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

use crate::components::{ChartMetric, ChartTab, LineChartPanel};
use crate::types::ConstructionPlan;

const SIMULATION_DT_SECONDS: u32 = 1;
const MAX_SIMULATION_TIME_SECONDS: u32 = 3600;
const SIMULATION_TICK_INTERVAL_MS: u64 = 50;

#[component]
pub fn SimulationPanel(plan: ConstructionPlan, on_close: EventHandler<()>) -> Element {
    let queue = use_memo(move || plan.to_build_queue());

    let mut snapshots = use_signal(Vec::<EcoSnapshot>::new);
    let mut playing = use_signal(|| true);
    let mut finished = use_signal(|| false);
    let mut replay_counter = use_signal(|| 0u32);
    let mut ws = use_signal(|| None::<WebSocket>);

    // Open a WebSocket to the backend whenever the plan changes, replay is
    // requested, or play is resumed.
    use_effect(move || {
        // Subscribe to plan, play state, and replay counter.
        let queue = queue.read().clone();
        let is_playing = *playing.read();
        let _ = *replay_counter.read();

        // Close any existing socket before starting a new run.
        if let Some(old_ws) = ws.write().take() {
            let _ = old_ws.close();
        }

        if !is_playing {
            return;
        }

        snapshots.set(vec![]);
        finished.set(false);

        let new_ws = match WebSocket::new("/ws/simulate") {
            Ok(ws) => ws,
            Err(e) => {
                web_sys::console::error_1(&format!("failed to open websocket: {e:?}").into());
                finished.set(true);
                playing.set(false);
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
            let mut finished_signal = finished;
            let mut playing_signal = playing;
            let mut ws_signal = ws;
            move |event: MessageEvent| {
                let text = event.data().as_string().unwrap_or_default();
                match serde_json::from_str::<SimServerMessage>(&text) {
                    Ok(SimServerMessage::Started { .. }) => {
                        // Simulation has started; events will follow.
                    }
                    Ok(SimServerMessage::Event(SimulationEvent::Ticked(snapshot))) => {
                        snapshots_signal.with_mut(|v| v.push(snapshot));
                    }
                    Ok(SimServerMessage::Event(SimulationEvent::Finished)) => {
                        finished_signal.set(true);
                        playing_signal.set(false);
                        ws_signal.set(None);
                    }
                    Ok(SimServerMessage::Event(_)) => {}
                    Ok(SimServerMessage::ControlEvent(_)) => {
                        // Control events (e.g. state changes) are informational
                        // and do not affect the chart display.
                    }
                    Ok(SimServerMessage::Error { message }) => {
                        web_sys::console::error_1(&format!("simulation error: {message}").into());
                        finished_signal.set(true);
                        playing_signal.set(false);
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
            let mut finished_signal = finished;
            let mut playing_signal = playing;
            let mut ws_signal = ws;
            move |_event: CloseEvent| {
                // If the socket closes before we saw Finished, mark the run as
                // done so the UI stops waiting.
                if !*finished_signal.read() {
                    finished_signal.set(true);
                    playing_signal.set(false);
                }
                ws_signal.set(None);
            }
        }) as Box<dyn FnMut(CloseEvent)>);
        new_ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();
    });

    let snaps = snapshots.read();
    let current_time = snaps.last().map_or(0.0, |s| s.time);
    let is_playing = *playing.read();
    let is_finished = *finished.read();

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
                button {
                    class: "px-3 py-1.5 text-sm rounded bg-blue-700 hover:bg-blue-600 text-white transition-colors",
                    onclick: move |_| {
                        let current = *playing.read();
                        playing.set(!current);
                    },
                    if is_playing {
                        "Pause"
                    } else {
                        "Play"
                    }
                }
                button {
                    class: "px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    onclick: move |_| {
                        snapshots.set(vec![]);
                        finished.set(false);
                        playing.set(true);
                        replay_counter.with_mut(|c| *c = c.wrapping_add(1));
                    },
                    "Replay"
                }
                button {
                    class: "px-3 py-1.5 text-sm rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 transition-colors",
                    onclick: move |_| on_close.call(()),
                    "Close"
                }
                span { class: "text-sm text-neutral-400 tabular-nums ml-auto",
                    if is_finished {
                        "{current_time:.1}s / {current_time:.1}s"
                    } else {
                        "{current_time:.1}s / ..."
                    }
                }
            }
            LineChartPanel {
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
