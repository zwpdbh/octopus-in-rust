use dioxus::prelude::*;
use faf_dioxus_ui::Stat;
use faf_sim_protocol::EcoSnapshot;

#[component]
pub fn EcoStats(eco: Signal<Option<EcoSnapshot>>) -> Element {
    let (mass_income, mass_drain, energy_income, energy_drain, mass_storage, energy_storage) = eco
        .read()
        .as_ref()
        .map_or((None, None, None, None, None, None), |s| {
            (
                Some(format!("{:.1}", s.production_per_second_mass)),
                Some(format!("{:.1}", s.mass_drain)),
                Some(format!("{:.1}", s.production_per_second_energy)),
                Some(format!("{:.1}", s.energy_drain)),
                Some(format!("{:.1} / {:.1}", s.mass_storage, s.mass_storage_cap)),
                Some(format!(
                    "{:.1} / {:.1}",
                    s.energy_storage, s.energy_storage_cap
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
