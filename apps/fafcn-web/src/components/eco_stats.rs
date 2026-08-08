use dioxus::prelude::*;
use faf_sim_protocol::EcoSnapshot;

#[component]
pub fn EcoStats(eco: Signal<Option<EcoSnapshot>>) -> Element {
    let stats: Vec<(&str, String)> = eco
        .read()
        .as_ref()
        .map_or_else(
            || {
                vec![
                    ("Mass Income", "—".to_string()),
                    ("Mass Drain", "—".to_string()),
                    ("Mass Storage", "—".to_string()),
                    ("Energy Income", "—".to_string()),
                    ("Energy Drain", "—".to_string()),
                    ("Energy Storage", "—".to_string()),
                ]
            },
            |s| {
                vec![
                    ("Mass Income", format!("{:.1}", s.production_per_second_mass)),
                    ("Mass Drain", format!("{:.1}", s.mass_drain)),
                    (
                        "Mass Storage",
                        format!("{:.1} / {:.1}", s.mass_storage, s.mass_storage_cap),
                    ),
                    (
                        "Energy Income",
                        format!("{:.1}", s.production_per_second_energy),
                    ),
                    ("Energy Drain", format!("{:.1}", s.energy_drain)),
                    (
                        "Energy Storage",
                        format!("{:.1} / {:.1}", s.energy_storage, s.energy_storage_cap),
                    ),
                ]
            },
        );

    rsx! {
        div { class: "grid grid-cols-2 md:grid-cols-3 gap-2",
            for (label , value) in stats {
                div { class: "flex items-center justify-between gap-2 px-2 py-1 rounded bg-neutral-800/30 border border-neutral-800 text-xs",
                    span { class: "text-neutral-500 whitespace-nowrap", "{label}" }
                    span { class: "text-white font-medium tabular-nums text-right", "{value}" }
                }
            }
        }
    }
}
