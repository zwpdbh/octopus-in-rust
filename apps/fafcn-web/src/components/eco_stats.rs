use dioxus::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
use faf_dioxus_ui::Stat;

#[component]
pub fn EcoStats(eco: Signal<Option<PlayerEcoMetrics>>) -> Element {
    let (mass_income, mass_drain, energy_income, energy_drain, mass_storage, energy_storage) = eco
        .read()
        .as_ref()
        .map_or((None, None, None, None, None, None), |m| {
            (
                Some(format!("{:.1}", m.mass_generate_rate)),
                Some(format!("{:.1}", m.mass_drain)),
                Some(format!("{:.1}", m.energy_generate_rate)),
                Some(format!("{:.1}", m.energy_drain)),
                Some(format!(
                    "{:.1} / {:.1}",
                    m.mass_in_storage, m.max_capacity_in_mass_storage
                )),
                Some(format!(
                    "{:.1} / {:.1}",
                    m.energy_in_storage, m.max_capacity_in_energy_storage
                )),
            )
        });

    rsx! {
        div { class: "grid grid-cols-2 md:grid-cols-4 gap-3",
            Stat { label: "Mass Income".to_string(), value: mass_income }
            Stat { label: "Mass Drain".to_string(), value: mass_drain }
            Stat { label: "Energy Income".to_string(), value: energy_income }
            Stat { label: "Energy Drain".to_string(), value: energy_drain }
            Stat { label: "Mass Storage".to_string(), value: mass_storage }
            Stat { label: "Energy Storage".to_string(), value: energy_storage }
        }
    }
}
