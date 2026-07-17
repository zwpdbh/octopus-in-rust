use dioxus::prelude::*;

use crate::components::SliderField;
use crate::types::{EcoSnapshot, ScheduleApiRequest, SearchOptions, UnitKind};
use crate::utils::kind_label;

/// Which scheduling mode the form is configured for; mirrors the CLI's
/// `ScheduleMode::{Eco, Unit}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleModeTab {
    Eco,
    Unit,
}

/// Editable scheduling form state, converted into a [`ScheduleApiRequest`]
/// when the user hits Compute.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleFormState {
    pub mode: ScheduleModeTab,
    /// Eco mode: target mass income in /s (slider range 100–900).
    pub target_mass_production: f64,
    /// Eco mode: target tolerance.
    pub tolerance: f64,
    /// Unit mode: the target unit kind.
    pub unit_target: Option<UnitKind>,
    pub initial_mass_production: f64,
    pub initial_energy_production: f64,
    pub initial_mass_storage: f64,
    pub initial_energy_storage: f64,
    pub initial_inventory: Vec<UnitKind>,
    pub options: SearchOptions,
}

impl Default for ScheduleFormState {
    fn default() -> Self {
        Self {
            mode: ScheduleModeTab::Eco,
            target_mass_production: 240.0,
            tolerance: 1.0,
            unit_target: None,
            initial_mass_production: 1.0,
            initial_energy_production: 20.0,
            initial_mass_storage: 650.0,
            initial_energy_storage: 4000.0,
            initial_inventory: vec![UnitKind::Commander],
            options: SearchOptions::default(),
        }
    }
}

impl ScheduleFormState {
    fn initial_snapshot(&self) -> EcoSnapshot {
        EcoSnapshot {
            time: 0.0,
            production_per_second_mass: self.initial_mass_production,
            production_per_second_energy: self.initial_energy_production,
            maintenance_consumption_per_second_energy: 0.0,
            mass_drain: 0.0,
            energy_drain: 0.0,
            total_mass_spent: 0.0,
            total_energy_spent: 0.0,
            mass_storage: self.initial_mass_storage,
            mass_storage_cap: self.initial_mass_storage,
            energy_storage: self.initial_energy_storage,
            energy_storage_cap: self.initial_energy_storage,
        }
    }

    /// True when the form has enough information to compute a schedule.
    pub fn is_valid(&self) -> bool {
        match self.mode {
            ScheduleModeTab::Eco => self.target_mass_production > 0.0,
            ScheduleModeTab::Unit => self.unit_target.is_some(),
        }
    }

    /// Build the API request payload for the current form state.
    pub fn to_request(&self) -> ScheduleApiRequest {
        let initial_eco = self.initial_snapshot();
        let initial_inventory = self.initial_inventory.clone();
        let options = self.options.clone();
        match self.mode {
            ScheduleModeTab::Eco => ScheduleApiRequest::Eco {
                initial_eco,
                initial_inventory,
                target_mass_production: self.target_mass_production,
                tolerance: self.tolerance,
                options,
            },
            ScheduleModeTab::Unit => ScheduleApiRequest::Unit {
                initial_eco,
                initial_inventory,
                target: self
                    .unit_target
                    .clone()
                    .expect("unit target must be set before computing"),
                options,
            },
        }
    }
}

/// Left-column scheduling request form.
#[component]
pub fn ScheduleRequestPanel(
    mut form: Signal<ScheduleFormState>,
    /// Abstract unit kinds offered as targets/inventory entries.
    kinds: Vec<UnitKind>,
    /// True while a request is in flight.
    computing: bool,
    on_compute: EventHandler<()>,
) -> Element {
    let mode = form.read().mode;
    let valid = form.read().is_valid();

    rsx! {
        div { class: "flex flex-col gap-3 min-w-0 p-3 rounded-lg border border-neutral-700 bg-neutral-900/80",
            h3 { class: "text-sm font-semibold text-white", "Schedule Request" }

            // Mode tabs.
            div { class: "grid grid-cols-2 gap-1 p-1 rounded bg-neutral-950 border border-neutral-800",
                ModeTab {
                    label: "Eco Target",
                    active: mode == ScheduleModeTab::Eco,
                    onclick: move |_| form.write().mode = ScheduleModeTab::Eco,
                }
                ModeTab {
                    label: "Unit Target",
                    active: mode == ScheduleModeTab::Unit,
                    onclick: move |_| form.write().mode = ScheduleModeTab::Unit,
                }
            }

            // Algorithm (only greedy exists today).
            div { class: "flex items-center justify-between gap-2 text-sm",
                span { class: "text-neutral-400", "Algorithm" }
                span { class: "text-neutral-300 font-mono text-xs px-2 py-1 rounded bg-neutral-800 border border-neutral-700", "Greedy" }
            }

            match mode {
                ScheduleModeTab::Eco => rsx! {
                    SliderField {
                        label: "Target mass income",
                        value: form.read().target_mass_production,
                        min: 100.0,
                        max: 900.0,
                        unit: "/s",
                        disabled: computing,
                        on_change: move |v: f64| form.write().target_mass_production = v.clamp(100.0, 900.0),
                    }
                    NumberField {
                        label: "Tolerance",
                        value: form.read().tolerance,
                        step: 0.1,
                        disabled: computing,
                        on_change: move |v: f64| form.write().tolerance = v.max(0.0),
                    }
                },
                ScheduleModeTab::Unit => rsx! {
                    KindSelect {
                        label: "Target unit",
                        kinds: kinds.clone(),
                        selected: form.read().unit_target.clone(),
                        placeholder: "Select target unit...",
                        disabled: computing,
                        on_change: move |k: Option<UnitKind>| form.write().unit_target = k,
                    }
                },
            }

            // Initial conditions (collapsible).
            details { class: "rounded border border-neutral-800 bg-neutral-950/60",
                summary { class: "px-3 py-2 text-xs font-semibold text-neutral-300 cursor-pointer select-none", "Initial conditions" }
                div { class: "px-3 pb-3 flex flex-col gap-3",
                    SliderField {
                        label: "Mass production",
                        value: form.read().initial_mass_production,
                        min: 1.0,
                        max: 200.0,
                        unit: "/s",
                        disabled: computing,
                        on_change: move |v: f64| form.write().initial_mass_production = v.clamp(1.0, 200.0),
                    }
                    SliderField {
                        label: "Energy production",
                        value: form.read().initial_energy_production,
                        min: 20.0,
                        max: 2000.0,
                        unit: "/s",
                        disabled: computing,
                        on_change: move |v: f64| form.write().initial_energy_production = v.clamp(20.0, 2000.0),
                    }
                    SliderField {
                        label: "Mass storage",
                        value: form.read().initial_mass_storage,
                        min: 0.0,
                        max: 2000.0,
                        unit: "",
                        disabled: computing,
                        on_change: move |v: f64| form.write().initial_mass_storage = v.clamp(0.0, 2000.0),
                    }
                    SliderField {
                        label: "Energy storage",
                        value: form.read().initial_energy_storage,
                        min: 0.0,
                        max: 10000.0,
                        unit: "",
                        disabled: computing,
                        on_change: move |v: f64| form.write().initial_energy_storage = v.clamp(0.0, 10000.0),
                    }
                    InventoryEditor { form, kinds: kinds.clone(), disabled: computing }
                }
            }

            // Advanced options (collapsed by default).
            details { class: "rounded border border-neutral-800 bg-neutral-950/60",
                summary { class: "px-3 py-2 text-xs font-semibold text-neutral-300 cursor-pointer select-none", "Advanced" }
                div { class: "px-3 pb-3 flex flex-col gap-3",
                    NumberField {
                        label: "Max search time (s)",
                        value: form.read().options.max_search_seconds,
                        step: 0.5,
                        disabled: computing,
                        on_change: move |v: f64| form.write().options.max_search_seconds = v.max(0.1),
                    }
                    NumberField {
                        label: "Max sim time (s)",
                        value: form.read().options.simulation_max_time_seconds,
                        step: 60.0,
                        disabled: computing,
                        on_change: move |v: f64| form.write().options.simulation_max_time_seconds = v.max(60.0),
                    }
                }
            }

            button {
                class: if valid && !computing {
                    "mt-1 px-3 py-2 rounded bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold transition-colors"
                } else {
                    "mt-1 px-3 py-2 rounded bg-neutral-700 text-neutral-400 text-sm font-semibold cursor-not-allowed"
                },
                disabled: !valid || computing,
                onclick: move |_| on_compute.call(()),
                if computing { "⚡ Computing..." } else { "⚡ Compute Schedule" }
            }
        }
    }
}

#[component]
fn ModeTab(label: &'static str, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if active {
                "px-2 py-1.5 text-xs font-semibold rounded bg-blue-700 text-white transition-colors"
            } else {
                "px-2 py-1.5 text-xs font-semibold rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
            },
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

/// A compact labelled number input used for tolerance and advanced options.
#[component]
fn NumberField(
    label: &'static str,
    value: f64,
    step: f64,
    #[props(default = false)] disabled: bool,
    on_change: EventHandler<f64>,
) -> Element {
    rsx! {
        label { class: "flex items-center justify-between gap-2 text-sm",
            span { class: "text-neutral-400", "{label}" }
            input {
                r#type: "number",
                class: "w-24 px-2 py-1 text-xs font-mono rounded bg-neutral-950 border border-neutral-700 text-neutral-200 focus:outline-none focus:border-blue-500",
                value: "{value}",
                step: "{step}",
                disabled: disabled,
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        on_change.call(v);
                    }
                },
            }
        }
    }
}

/// A `<select>` of abstract unit kinds. The selected kind travels through the
/// option value as serialized JSON to avoid any string grammar.
#[component]
fn KindSelect(
    label: &'static str,
    kinds: Vec<UnitKind>,
    selected: Option<UnitKind>,
    placeholder: &'static str,
    #[props(default = false)] disabled: bool,
    on_change: EventHandler<Option<UnitKind>>,
) -> Element {
    let selected_json = selected
        .as_ref()
        .and_then(|k| serde_json::to_string(k).ok())
        .unwrap_or_default();

    rsx! {
        label { class: "flex flex-col gap-1 text-sm",
            span { class: "text-neutral-400", "{label}" }
            select {
                class: "px-2 py-1.5 text-xs rounded bg-neutral-950 border border-neutral-700 text-neutral-200 focus:outline-none focus:border-blue-500",
                disabled: disabled,
                value: "{selected_json}",
                onchange: move |e| {
                    let raw = e.value();
                    let parsed = serde_json::from_str::<UnitKind>(&raw).ok();
                    on_change.call(parsed);
                },
                option { value: "", "{placeholder}" }
                for kind in kinds.iter() {
                    {
                        let json = serde_json::to_string(kind).unwrap_or_default();
                        rsx! {
                            option { value: "{json}", "{kind_label(kind)}" }
                        }
                    }
                }
            }
        }
    }
}

/// Tag-list editor for the initial inventory.
#[component]
fn InventoryEditor(
    mut form: Signal<ScheduleFormState>,
    kinds: Vec<UnitKind>,
    disabled: bool,
) -> Element {
    let inventory = form.read().initial_inventory.clone();

    rsx! {
        div { class: "flex flex-col gap-1 text-sm",
            span { class: "text-neutral-400", "Initial inventory" }
            div { class: "flex flex-wrap gap-1.5",
                for kind in inventory.iter() {
                    {
                        let k = kind.clone();
                        rsx! {
                            span { class: "inline-flex items-center gap-1 px-2 py-0.5 rounded bg-neutral-800 border border-neutral-700 text-xs text-neutral-200",
                                "{kind_label(kind)}"
                                button {
                                    class: "text-neutral-500 hover:text-red-400 leading-none",
                                    disabled: disabled,
                                    onclick: move |_| {
                                        form.write().initial_inventory.retain(|x| x != &k);
                                    },
                                    "×"
                                }
                            }
                        }
                    }
                }
            }
            {
                let remaining: Vec<UnitKind> = kinds
                    .iter()
                    .filter(|k| !inventory.contains(k))
                    .cloned()
                    .collect();
                rsx! {
                    KindSelect {
                        label: "Add unit",
                        kinds: remaining,
                        selected: None,
                        placeholder: "Add to inventory...",
                        disabled: disabled,
                        on_change: move |k: Option<UnitKind>| {
                            if let Some(k) = k {
                                let mut f = form.write();
                                if !f.initial_inventory.contains(&k) {
                                    f.initial_inventory.push(k);
                                }
                            }
                        },
                    }
                }
            }
        }
    }
}
