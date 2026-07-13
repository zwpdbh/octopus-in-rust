use dioxus::prelude::*;
use faf_sim::{Energy, EnergyRate, Mass, MassRate, Storage};

use crate::components::SliderField;
use crate::types::{ConstructionPlan, EcoInitialSettings};

#[component]
pub fn EcoPanel(
    mut plan: Signal<ConstructionPlan>,
    #[props(default = false)] disabled: bool,
) -> Element {
    fn update<F: FnOnce(&mut EcoInitialSettings)>(mut plan: Signal<ConstructionPlan>, f: F) {
        plan.with_mut(|p| f(&mut p.eco));
    }

    rsx! {
        div {
            class: "flex flex-col gap-3 min-w-[260px] p-3 rounded-lg border border-neutral-700 bg-neutral-900/80",
            h3 { class: "text-sm font-semibold text-white", "Eco Settings" }
            SliderField {
                label: "Mass Income",
                value: plan.read().eco.mass_income.value(),
                min: 1.0,
                max: 200.0,
                unit: "",
                disabled: disabled,
                on_change: move |v: f64| update(plan, |eco| eco.mass_income = MassRate::from_raw(v.clamp(1.0, 200.0))),
            }
            SliderField {
                label: "Energy Income",
                value: plan.read().eco.energy_income.value(),
                min: 20.0,
                max: 2000.0,
                unit: "",
                disabled: disabled,
                on_change: move |v: f64| update(plan, |eco| eco.energy_income = EnergyRate::from_raw(v.clamp(20.0, 2000.0))),
            }
            SliderField {
                label: "Mass Storage",
                value: plan.read().eco.mass_storage.current.value(),
                min: 0.0,
                max: 650.0,
                unit: "",
                disabled: disabled,
                on_change: move |v: f64| {
                    let amount = Mass::from_raw(v.clamp(0.0, 650.0));
                    update(plan, |eco| eco.mass_storage = Storage::new(amount, amount));
                },
            }
            SliderField {
                label: "Energy Storage",
                value: plan.read().eco.energy_storage.current.value(),
                min: 0.0,
                max: 4000.0,
                unit: "",
                disabled: disabled,
                on_change: move |v: f64| {
                    let amount = Energy::from_raw(v.clamp(0.0, 4000.0));
                    update(plan, |eco| eco.energy_storage = Storage::new(amount, amount));
                },
            }
        }
    }
}
