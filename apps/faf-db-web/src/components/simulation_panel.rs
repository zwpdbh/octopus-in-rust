use dioxus::prelude::*;
use faf_sim::protocol::{SimClientMessage, SimServerMessage};
use faf_sim::sim::{EcoSnapshot, SimulationEvent};
use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

use crate::types::ConstructionPlan;

const MASS_INCOME_CANVAS: &str = "mass-income-chart";
const ENERGY_INCOME_CANVAS: &str = "energy-income-chart";
const TOTAL_MASS_CANVAS: &str = "total-mass-chart";
const TOTAL_ENERGY_CANVAS: &str = "total-energy-chart";

const SIMULATION_RESOLUTION: u32 = 10;
const MAX_SIMULATION_TIME: f64 = 3600.0;

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
                    resolution: SIMULATION_RESOLUTION,
                    max_time: Some(MAX_SIMULATION_TIME),
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
                    Ok(SimServerMessage::Event(SimulationEvent::Ticked(snapshot))) => {
                        snapshots_signal.with_mut(|v| v.push(snapshot));
                    }
                    Ok(SimServerMessage::Event(SimulationEvent::Finished)) => {
                        finished_signal.set(true);
                        playing_signal.set(false);
                        ws_signal.set(None);
                    }
                    Ok(SimServerMessage::Event(_)) => {}
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

    // Redraw charts whenever the snapshot list grows.
    use_effect(move || {
        let snaps = snapshots.read();
        if snaps.len() < 2 {
            return;
        }
        draw_chart(
            MASS_INCOME_CANVAS,
            &snaps,
            |s| s.mass_income,
            RGBColor(59, 130, 246),
        );
        draw_chart(
            ENERGY_INCOME_CANVAS,
            &snaps,
            |s| s.energy_income,
            RGBColor(234, 179, 8),
        );
        draw_chart(
            TOTAL_MASS_CANVAS,
            &snaps,
            |s| s.total_mass_spent,
            RGBColor(34, 197, 94),
        );
        draw_chart(
            TOTAL_ENERGY_CANVAS,
            &snaps,
            |s| s.total_energy_spent,
            RGBColor(249, 115, 22),
        );
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
            if snaps.len() < 2 {
                p { class: "text-sm text-neutral-500 text-center mt-4", "Simulating..." }
            } else {
                div { class: "flex-1 overflow-auto",
                    div { class: "grid grid-cols-1 md:grid-cols-2 gap-3",
                        MetricCard {
                            title: "Mass income",
                            color: "bg-blue-500",
                            canvas_id: MASS_INCOME_CANVAS,
                        }
                        MetricCard {
                            title: "Energy income",
                            color: "bg-yellow-500",
                            canvas_id: ENERGY_INCOME_CANVAS,
                        }
                        MetricCard {
                            title: "Total mass spent",
                            color: "bg-green-500",
                            canvas_id: TOTAL_MASS_CANVAS,
                        }
                        MetricCard {
                            title: "Total energy spent",
                            color: "bg-orange-500",
                            canvas_id: TOTAL_ENERGY_CANVAS,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MetricCard(title: String, color: String, canvas_id: String) -> Element {
    rsx! {
        div { class: "rounded-lg border border-neutral-800 bg-[#171717] p-2",
            div { class: "flex items-center gap-2 mb-1",
                div { class: "w-3 h-3 rounded-full {color}" }
                h2 { class: "text-sm font-semibold text-white", "{title}" }
            }
            canvas {
                id: "{canvas_id}",
                width: "400",
                height: "240",
                class: "w-full h-auto rounded border border-neutral-800",
            }
        }
    }
}

fn draw_chart(
    canvas_id: &str,
    snapshots: &[EcoSnapshot],
    metric: fn(&EcoSnapshot) -> f64,
    color: RGBColor,
) {
    let backend = match CanvasBackend::new(canvas_id) {
        Some(b) => b,
        None => return,
    };
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(23, 23, 23)).unwrap();

    if snapshots.len() < 2 {
        root.present().unwrap();
        return;
    }

    let full_data: Vec<(f64, f64)> = snapshots.iter().map(|s| (s.time, metric(s))).collect();
    let max_time = snapshots.last().unwrap().time.max(1.0);
    let (min_y, max_y) = range_for_series(&full_data);

    let mut chart = ChartBuilder::on(&root)
        .margin(8)
        .x_label_area_size(0)
        .y_label_area_size(0)
        .build_cartesian_2d(0.0..max_time, min_y..max_y)
        .unwrap();

    chart
        .configure_mesh()
        .x_labels(0)
        .y_labels(0)
        .light_line_style(RGBColor(60, 60, 60))
        .draw()
        .unwrap();

    chart
        .draw_series(LineSeries::new(full_data, &color))
        .unwrap();

    root.present().unwrap();
}

fn range_for_series(data: &[(f64, f64)]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for (_, y) in data {
        min = min.min(*y);
        max = max.max(*y);
    }
    if min.is_infinite() || max.is_infinite() || min == max {
        return (0.0, 1.0);
    }
    let padding = (max - min) * 0.05;
    let min = (min - padding).max(0.0);
    let max = max + padding;
    if max <= min {
        return (min, min + 1.0);
    }
    (min, max)
}
