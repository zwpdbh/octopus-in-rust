use dioxus::prelude::*;

use crate::types::EcoSnapshot;

/// Game-style economy panel showing mass and energy storage, net rate, and
/// income/expense breakdown.
///
/// The central net-rate value is clickable: it toggles between the absolute
/// net rate (e.g. `+5.0/s`) and an efficiency percentage
/// (`income / expense * 100`, e.g. `125%`).
#[component]
pub fn EcoSnapshotView(snapshot: EcoSnapshot, #[props(default = false)] compact: bool) -> Element {
    let show_pct = use_signal(|| false);
    let padding = if compact { "p-2" } else { "p-3" };
    let gap = if compact { "gap-1.5" } else { "gap-2" };
    let icon_size = if compact { "w-6 h-6 text-xs" } else { "w-7 h-7 text-sm" };

    rsx! {
        div { class: "rounded border border-neutral-700 bg-neutral-900/80 {padding} flex flex-col {gap} select-none",
            ResourceRow {
                label: "M",
                label_class: "bg-emerald-600 text-white",
                bar_class: "bg-emerald-500",
                positive_class: "text-emerald-400",
                icon_size,
                compact,
                income: snapshot.production_per_second_mass.value(),
                expense: snapshot.mass_drain.value(),
                storage: snapshot.mass_storage.value(),
                cap: snapshot.mass_storage_cap.value(),
                show_pct,
            }
            ResourceRow {
                label: "E",
                label_class: "bg-amber-500 text-black",
                bar_class: "bg-amber-400",
                positive_class: "text-amber-300",
                icon_size,
                compact,
                income: snapshot.production_per_second_energy.value(),
                expense: snapshot.energy_drain.value() + snapshot.maintenance_consumption_per_second_energy.value(),
                storage: snapshot.energy_storage.value(),
                cap: snapshot.energy_storage_cap.value(),
                show_pct,
            }
        }
    }
}

#[component]
fn ResourceRow(
    label: &'static str,
    label_class: &'static str,
    bar_class: &'static str,
    positive_class: &'static str,
    icon_size: &'static str,
    compact: bool,
    income: f64,
    expense: f64,
    storage: f64,
    cap: f64,
    show_pct: Signal<bool>,
) -> Element {
    let net = income - expense;
    let ratio = if cap > 0.0 { storage / cap } else { 0.0 };
    let storage_pct = ratio * 100.0;
    let negative = net < -0.001;

    let net_text = if *show_pct.read() {
        if expense > 0.001 {
            format!("{:.0}%", (income / expense) * 100.0)
        } else if income > 0.001 {
            "∞".to_string()
        } else {
            "0%".to_string()
        }
    } else {
        format!("{:+.1}/s", net)
    };

    let net_color = if negative { "text-red-400" } else { positive_class };
    let storage_text = format!("{:.0}/{:.0}", storage, cap);
    let text_size = if compact { "text-xs" } else { "text-sm" };
    let bar_height = if compact { "h-2.5" } else { "h-3.5" };
    let number_width = if compact { "w-16" } else { "w-20" };

    rsx! {
        div { class: "grid items-center gap-3",
            style: "grid-template-columns: auto 1fr auto auto",
            // Resource icon.
            div { class: "{icon_size} flex items-center justify-center rounded font-bold {label_class}",
                "{label}"
            }

            // Storage bar + value.
            div { class: "min-w-0 flex items-center gap-2",
                div { class: "flex-1 {bar_height} rounded bg-neutral-800 overflow-hidden",
                    div {
                        class: "h-full {bar_class}",
                        style: "width: {storage_pct.min(100.0):.1}%",
                    }
                }
                span { class: "{text_size} font-mono text-neutral-300 {number_width} text-right shrink-0",
                    "{storage_text}"
                }
            }

            // Net rate (clickable toggle).
            button {
                class: "{text_size} {number_width} text-right font-mono hover:opacity-80 transition-opacity {net_color}",
                title: "Click to toggle absolute rate / efficiency percentage",
                onclick: move |_| {
                    let current = *show_pct.read();
                    show_pct.set(!current);
                },
                "{net_text}"
            }

            // Income / expense breakdown.
            div { class: "{text_size} font-mono leading-tight w-20 text-right shrink-0",
                div { class: "text-emerald-400", "+{income:.1}/s" }
                div { class: "text-red-400", "-{expense:.1}/s" }
            }
        }
    }
}
