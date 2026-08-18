use dioxus::prelude::*;
use faf_sim_protocol::EcoSnapshot;

use crate::i18n::{self, Text};

#[component]
pub fn EcoStats(eco: Signal<Option<EcoSnapshot>>) -> Element {
    let t = i18n::use_t();
    let (mass_income, mass_drain, mass_storage, energy_income, energy_drain, energy_storage) = (
        t.t(Text::MassIncome),
        t.t(Text::MassDrain),
        t.t(Text::MassStorage),
        t.t(Text::EnergyIncome),
        t.t(Text::EnergyDrain),
        t.t(Text::EnergyStorage),
    );
    let stats: Vec<(&str, String)> = eco.read().as_ref().map_or_else(
        || {
            vec![
                (mass_income, "—".to_string()),
                (mass_drain, "—".to_string()),
                (mass_storage, "—".to_string()),
                (energy_income, "—".to_string()),
                (energy_drain, "—".to_string()),
                (energy_storage, "—".to_string()),
            ]
        },
        |s| {
            vec![
                (mass_income, format!("{:.1}", s.production_per_second_mass)),
                (mass_drain, format!("{:.1}", s.mass_drain)),
                (
                    mass_storage,
                    format!("{:.1} / {:.1}", s.mass_storage, s.mass_storage_cap),
                ),
                (
                    energy_income,
                    format!("{:.1}", s.production_per_second_energy),
                ),
                (energy_drain, format!("{:.1}", s.energy_drain)),
                (
                    energy_storage,
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
