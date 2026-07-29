use dioxus::prelude::*;
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage};
use faf_sim::GameEcoMetrics;

use crate::components::SliderField;
use crate::types::ConstructionPlan;

#[component]
pub fn EcoPanel(
    mut plan: Signal<ConstructionPlan>,
    #[props(default = false)] disabled: bool,
) -> Element {
    fn update<F: FnOnce(&mut GameEcoMetrics)>(mut plan: Signal<ConstructionPlan>, f: F) {
        plan.with_mut(|p| f(&mut p.eco));
    }

    rsx! {
        div { class: "flex flex-col gap-3 min-w-[260px] p-3 rounded-lg border border-neutral-700 bg-neutral-900/80",
            h3 { class: "text-sm font-semibold text-white", "Eco Settings" }
            SliderField {
                label: "Mass production",
                value: plan.read().eco.production_per_second_mass.value(),
                min: 1.0,
                max: 200.0,
                unit: "",
                disabled,
                on_change: move |v: f64| update(
                    plan,
                    |eco| eco.production_per_second_mass = MassRate::from_raw(v.clamp(1.0, 200.0)),
                ),
            }
            SliderField {
                label: "Energy production",
                value: plan.read().eco.production_energy_per_second.value(),
                min: 20.0,
                max: 2000.0,
                unit: "",
                disabled,
                on_change: move |v: f64| update(
                    plan,
                    |eco| {
                        eco.production_energy_per_second = EnergyRate::from_raw(
                            v.clamp(20.0, 2000.0),
                        );
                    },
                ),
            }
            SliderField {
                label: "Energy maintenance",
                value: plan.read().eco.maintenance_consumption_per_second_energy.value(),
                min: 0.0,
                max: 1000.0,
                unit: "",
                disabled,
                on_change: move |v: f64| update(
                    plan,
                    |eco| {
                        eco.maintenance_consumption_per_second_energy = EnergyRate::from_raw(
                            v.clamp(0.0, 1000.0),
                        );
                    },
                ),
            }
            SliderField {
                label: "Mass storage",
                value: plan.read().eco.mass_storage.current.value(),
                min: 0.0,
                max: 650.0,
                unit: "",
                disabled,
                on_change: move |v: f64| {
                    let amount = Mass::from_raw(v.clamp(0.0, 650.0));
                    update(plan, |eco| eco.mass_storage = Storage::new(amount, amount));
                },
            }
            SliderField {
                label: "Energy storage",
                value: plan.read().eco.energy_storage.current.value(),
                min: 0.0,
                max: 4000.0,
                unit: "",
                disabled,
                on_change: move |v: f64| {
                    let amount = Energy::from_raw(v.clamp(0.0, 4000.0));
                    update(plan, |eco| eco.energy_storage = Storage::new(amount, amount));
                },
            }
        }
    }
}
