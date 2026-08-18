use dioxus::prelude::*;
use faf_blueprints::{ConstructionPlan, PlayerEcoMetrics};

use faf_dioxus_ui::SliderField;

use crate::i18n::{self, Text};

#[component]
pub fn EcoPanel(
    plan: Signal<ConstructionPlan>,
    #[props(default = false)] disabled: bool,
) -> Element {
    fn update<F: FnOnce(&mut PlayerEcoMetrics)>(mut plan: Signal<ConstructionPlan>, f: F) {
        plan.with_mut(|p| {
            let mut eco = *p.player_eco();
            f(&mut eco);
            p.set_player_eco(eco);
        });
    }

    let t = i18n::use_t();

    rsx! {
        div { class: "flex flex-col gap-3 min-w-[260px] p-3 rounded-lg border border-neutral-700 bg-neutral-900/80",
            h3 { class: "text-sm font-semibold text-white", "{t.t(Text::EcoSettings)}" }
            SliderField {
                label: t.t(Text::MassProduction).to_string(),
                value: plan.read().player_eco().mass_generate_rate,
                min: 1.0,
                max: 200.0,
                unit: "".to_string(),
                disabled,
                on_change: move |v: f64| update(plan, |eco| eco.mass_generate_rate = v.clamp(1.0, 200.0)),
            }
            SliderField {
                label: t.t(Text::EnergyProduction).to_string(),
                value: plan.read().player_eco().energy_generate_rate,
                min: 20.0,
                max: 2000.0,
                unit: "".to_string(),
                disabled,
                on_change: move |v: f64| update(plan, |eco| eco.energy_generate_rate = v.clamp(20.0, 2000.0)),
            }
            SliderField {
                label: t.t(Text::MassStorage).to_string(),
                value: plan.read().player_eco().mass_in_storage,
                min: 0.0,
                max: 10000.0,
                unit: "".to_string(),
                disabled,
                on_change: move |v: f64| update(plan, |eco| {
                    eco.mass_in_storage = v.clamp(0.0, 10000.0);
                    eco.max_capacity_in_mass_storage = eco.mass_in_storage;
                }),
            }
            SliderField {
                label: t.t(Text::EnergyStorage).to_string(),
                value: plan.read().player_eco().energy_in_storage,
                min: 0.0,
                max: 50000.0,
                unit: "".to_string(),
                disabled,
                on_change: move |v: f64| update(plan, |eco| {
                    eco.energy_in_storage = v.clamp(0.0, 50000.0);
                    eco.max_capacity_in_energy_storage = eco.energy_in_storage;
                }),
            }
        }
    }
}
