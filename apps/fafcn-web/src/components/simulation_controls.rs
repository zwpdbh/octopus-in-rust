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
    on_stop: EventHandler<()>,
    on_reset: EventHandler<()>,
) -> Element {
    let start_enabled =
        *status.read() == SimulationStatus::Idle || *status.read() == SimulationStatus::Finished;
    let pause_enabled = *status.read() == SimulationStatus::Running;
    let resume_enabled = *status.read() == SimulationStatus::Paused;
    let stop_enabled = *status.read() == SimulationStatus::Running
        || *status.read() == SimulationStatus::Paused
        || *status.read() == SimulationStatus::Finished;
    let reset_enabled = *status.read() != SimulationStatus::Idle;

    rsx! {
        div { class: "flex items-center gap-3",
            ControlButton {
                label: "Start",
                enabled: start_enabled,
                onclick: on_start,
                active_class: "bg-emerald-700 hover:bg-emerald-600",
            }
            ControlButton {
                label: "Pause",
                enabled: pause_enabled,
                onclick: on_pause,
                active_class: "bg-amber-700 hover:bg-amber-600",
            }
            ControlButton {
                label: "Resume",
                enabled: resume_enabled,
                onclick: on_resume,
                active_class: "bg-blue-700 hover:bg-blue-600",
            }
            ControlButton {
                label: "Stop",
                enabled: stop_enabled,
                onclick: on_stop,
                active_class: "bg-red-700 hover:bg-red-600",
            }
            ControlButton {
                label: "Reset",
                enabled: reset_enabled,
                onclick: on_reset,
                active_class: "bg-neutral-700 hover:bg-neutral-600",
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

#[component]
fn ControlButton(
    label: String,
    enabled: bool,
    onclick: EventHandler<()>,
    active_class: &'static str,
) -> Element {
    let base = "px-4 py-2 rounded text-white text-sm transition-colors";
    let disabled_class = "bg-neutral-800 text-neutral-500 cursor-not-allowed";
    let class = if enabled {
        format!("{base} {active_class}")
    } else {
        format!("{base} {disabled_class}")
    };

    rsx! {
        button {
            class,
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
