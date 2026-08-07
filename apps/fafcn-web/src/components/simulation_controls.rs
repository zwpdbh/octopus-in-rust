use dioxus::prelude::*;
use faf_sim_protocol::SimSpeed;

#[derive(Clone, Copy, PartialEq)]
pub enum SimulationStatus {
    Idle,
    Running,
    Paused,
    Finished,
}

#[component]
pub fn SimulationControls(
    status: Signal<SimulationStatus>,
    speed: Signal<f64>,
    on_start: EventHandler<()>,
    on_pause: EventHandler<()>,
    on_resume: EventHandler<()>,
    on_reset: EventHandler<()>,
) -> Element {
    let can_start =
        *status.read() == SimulationStatus::Idle || *status.read() == SimulationStatus::Finished;
    let is_running = *status.read() == SimulationStatus::Running;
    let is_paused = *status.read() == SimulationStatus::Paused;

    rsx! {
        div { class: "flex items-center gap-3",
            if can_start {
                button {
                    class: "px-4 py-2 rounded bg-emerald-700 hover:bg-emerald-600 text-white text-sm transition-colors",
                    onclick: move |_| on_start.call(()),
                    "Start"
                }
            }
            if is_running {
                button {
                    class: "px-4 py-2 rounded bg-amber-700 hover:bg-amber-600 text-white text-sm transition-colors",
                    onclick: move |_| on_pause.call(()),
                    "Pause"
                }
            }
            if is_paused {
                button {
                    class: "px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm transition-colors",
                    onclick: move |_| on_resume.call(()),
                    "Resume"
                }
            }
            if !can_start {
                button {
                    class: "px-4 py-2 rounded bg-neutral-700 hover:bg-neutral-600 text-white text-sm transition-colors",
                    onclick: move |_| on_reset.call(()),
                    "Reset"
                }
            }

            div { class: "flex items-center gap-2 ml-4",
                label { class: "text-xs text-neutral-400", "Speed" }
                select {
                    class: "bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-sm text-white",
                    onchange: move |evt| {
                        if let Ok(v) = evt.value().parse::<f64>() {
                            speed.set(v);
                        }
                    },
                    option { value: "0", selected: speed() == 0.0, "Unlimited" }
                    option { value: "1", selected: speed() == 1.0, "1x" }
                    option { value: "2", selected: speed() == 2.0, "2x" }
                    option { value: "5", selected: speed() == 5.0, "5x" }
                    option { value: "10", selected: speed() == 10.0, "10x" }
                }
            }
        }
    }
}

/// Convert a positive speed value into a `SimSpeed`.
///
/// Values `<= 0` map to `Unlimited`.
pub fn to_sim_speed(speed: f64) -> SimSpeed {
    if speed > 0.0 {
        SimSpeed::TicksPerSecond(speed)
    } else {
        SimSpeed::Unlimited
    }
}
