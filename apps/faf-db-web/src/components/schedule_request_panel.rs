use std::collections::HashMap;

use dioxus::prelude::*;
use faf_quantities::MassRate;

use crate::components::{SliderField, UnitSelectorModal};
use crate::types::{
    AssignmentTarget, EcoSnapshot, ScheduleApiRequest, SearchOptions, UnitKind, UnitSummary,
};
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
    /// Eco mode: target mass income in /s (positive integer, stepper ±20).
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
    pub max_mex_count: u32,
}

impl Default for ScheduleFormState {
    fn default() -> Self {
        Self {
            mode: ScheduleModeTab::Eco,
            target_mass_production: 50.0,
            tolerance: 1.0,
            unit_target: None,
            initial_mass_production: 1.0,
            initial_energy_production: 20.0,
            initial_mass_storage: 650.0,
            initial_energy_storage: 4000.0,
            initial_inventory: vec![UnitKind::Commander],
            options: SearchOptions::default(),
            max_mex_count: 10u32,
        }
    }
}

impl ScheduleFormState {
    pub fn initial_snapshot(&self) -> EcoSnapshot {
        use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Time};

        EcoSnapshot {
            time: Time::from_raw(0.0),
            production_per_second_mass: MassRate::from_raw(self.initial_mass_production),
            production_per_second_energy: EnergyRate::from_raw(self.initial_energy_production),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_drain: MassRate::from_raw(0.0),
            energy_drain: EnergyRate::from_raw(0.0),
            total_mass_spent: Mass::from_raw(0.0),
            total_energy_spent: Energy::from_raw(0.0),
            mass_storage: Mass::from_raw(self.initial_mass_storage),
            mass_storage_cap: Mass::from_raw(self.initial_mass_storage),
            energy_storage: Energy::from_raw(self.initial_energy_storage),
            energy_storage_cap: Energy::from_raw(self.initial_energy_storage),
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
                target_mass_production: MassRate::from_raw(self.target_mass_production),
                tolerance: self.tolerance,
                options,
                max_mex_count: self.max_mex_count,
            },
            ScheduleModeTab::Unit => ScheduleApiRequest::Unit {
                initial_eco,
                initial_inventory,
                target: self
                    .unit_target
                    .clone()
                    .expect("unit target must be set before computing"),
                options,
                max_mex_count: self.max_mex_count,
            },
        }
    }
}

/// Which form slot the unit picker modal is currently filling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingSlot {
    UnitTarget,
    AddInventory,
}

/// Left-column scheduling request form.
#[component]
pub fn ScheduleRequestPanel(
    mut form: Signal<ScheduleFormState>,
    /// Units offered in the picker modal (from the blueprint graph summaries).
    candidates: Vec<UnitSummary>,
    /// Lookup from a candidate's blueprint id to its abstract unit kind.
    id_to_kind: HashMap<String, UnitKind>,
    /// True while a schedule is being computed.
    computing: bool,
    on_compute: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let mut pending_slot = use_signal(|| None::<PendingSlot>);

    let mode = form.read().mode;
    let valid = form.read().is_valid();

    let id_to_kind_for_pick = id_to_kind.clone();
    let on_pick = move |summary: UnitSummary| {
        let kind = id_to_kind_for_pick.get(&summary.id).cloned();
        if let (Some(slot), Some(kind)) = (*pending_slot.read(), kind) {
            match slot {
                PendingSlot::UnitTarget => form.write().unit_target = Some(kind),
                PendingSlot::AddInventory => {
                    let mut f = form.write();
                    if !f.initial_inventory.contains(&kind) {
                        f.initial_inventory.push(kind);
                    }
                }
            }
        }
        pending_slot.set(None);
    };

    let target_summary = form.read().unit_target.as_ref().and_then(|kind| {
        candidates
            .iter()
            .find(|u| id_to_kind.get(&u.id) == Some(kind))
            .cloned()
    });

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
                    IntegerStepper {
                        label: "Target mass income",
                        value: form.read().target_mass_production,
                        step: 20.0,
                        disabled: computing,
                        on_change: move |v: f64| form.write().target_mass_production = v.max(1.0),
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
                    UnitSlot {
                        label: "Target unit",
                        summary: target_summary,
                        hint: "Click to select",
                        disabled: computing,
                        on_click: move |_| pending_slot.set(Some(PendingSlot::UnitTarget)),
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
                    InventoryEditor {
                        form,
                        id_to_kind,
                        candidates: candidates.clone(),
                        disabled: computing,
                        on_add: move |_| pending_slot.set(Some(PendingSlot::AddInventory)),
                    }
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
                    NumberField {
                        label: "Max mex count",
                        value: form.read().max_mex_count as f64,
                        step: 1.0,
                        disabled: computing,
                        on_change: move |v: f64| form.write().max_mex_count = v.max(0.0) as u32,
                    }
                }
            }

            if computing {
                div { class: "flex gap-2 mt-1",
                    button {
                        class: "flex-1 px-3 py-2 rounded bg-red-600 hover:bg-red-500 text-white text-sm font-semibold transition-colors",
                        onclick: move |_| on_cancel.call(()),
                        "Cancel"
                    }
                }
            } else {
                button {
                    class: if valid {
                        "mt-1 px-3 py-2 rounded bg-emerald-600 hover:bg-emerald-500 text-white text-sm font-semibold transition-colors"
                    } else {
                        "mt-1 px-3 py-2 rounded bg-neutral-700 text-neutral-400 text-sm font-semibold cursor-not-allowed"
                    },
                    disabled: !valid,
                    onclick: move |_| on_compute.call(()),
                    "⚡ Compute Schedule"
                }
            }
        }

        UnitSelectorModal {
            open: pending_slot.read().is_some(),
            units: candidates.clone(),
            target: AssignmentTarget::NewTarget,
            on_select: on_pick,
            on_close: move |_| pending_slot.set(None),
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

/// A compact labelled integer input with up/down stepper buttons.
#[component]
fn IntegerStepper(
    label: &'static str,
    value: f64,
    #[props(default = 20.0)] step: f64,
    #[props(default = false)] disabled: bool,
    on_change: EventHandler<f64>,
) -> Element {
    let display = format!("{:.0}", value.max(1.0));
    rsx! {
        label { class: "flex items-center justify-between gap-2 text-sm",
            span { class: "text-neutral-400", "{label}" }
            div { class: "flex items-stretch",
                input {
                    r#type: "text",
                    inputmode: "numeric",
                    pattern: "[0-9]*",
                    class: "w-20 px-2 py-1 text-xs font-mono rounded-l bg-neutral-950 border border-neutral-700 text-neutral-200 focus:outline-none focus:border-blue-500",
                    value: "{display}",
                    disabled: disabled,
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<i64>() {
                            on_change.call(v.max(1) as f64);
                        }
                    },
                }
                div { class: "flex flex-col border border-l-0 border-neutral-700 rounded-r overflow-hidden",
                    button {
                        class: "flex-1 px-1.5 text-[10px] leading-none bg-neutral-800 text-neutral-300 hover:bg-neutral-700 disabled:opacity-50 disabled:cursor-not-allowed",
                        disabled: disabled,
                        onclick: move |_| on_change.call((value + step).max(1.0)),
                        "▲"
                    }
                    button {
                        class: "flex-1 px-1.5 text-[10px] leading-none bg-neutral-800 text-neutral-300 hover:bg-neutral-700 disabled:opacity-50 disabled:cursor-not-allowed",
                        disabled: disabled,
                        onclick: move |_| on_change.call((value - step).max(1.0)),
                        "▼"
                    }
                }
            }
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

/// A "?" slot button that opens the unit picker modal, styled like the
/// `UnitBlock` used on the simulate build page.
#[component]
fn UnitSlot(
    label: &'static str,
    summary: Option<UnitSummary>,
    hint: &'static str,
    #[props(default = false)] disabled: bool,
    on_click: EventHandler<()>,
) -> Element {
    let button_class = if disabled {
        "w-16 h-16 p-1 rounded bg-black border border-neutral-700 flex items-center justify-center self-center cursor-not-allowed opacity-60"
    } else {
        "w-16 h-16 p-1 rounded bg-black border border-neutral-600 flex items-center justify-center transition-colors hover:border-neutral-400 self-center"
    };
    rsx! {
        div { class: "flex flex-col gap-2 p-2 rounded bg-neutral-800/50 border border-neutral-700",
            span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
            button {
                class: "{button_class}",
                disabled: disabled,
                onclick: move |_| {
                    if !disabled {
                        on_click.call(());
                    }
                },
                title: "Click to select a unit",
                if let Some(ref u) = summary {
                    img {
                        src: "/api/portraits/{u.id}.png",
                        alt: "{u.display_name}",
                        class: "w-full h-full object-contain",
                    }
                } else {
                    span { class: "text-neutral-500 text-3xl", "?" }
                }
            }
            div { class: "flex flex-col items-center text-center gap-1",
                span { class: "text-sm text-neutral-300 truncate w-full",
                    {summary.as_ref().map(|u| u.display_name.as_str()).unwrap_or("—")}
                }
                span { class: "text-[10px] text-neutral-500", "{hint}" }
            }
        }
    }
}

/// Tag-list editor for the initial inventory, plus a "?" slot to add units.
#[component]
fn InventoryEditor(
    mut form: Signal<ScheduleFormState>,
    id_to_kind: HashMap<String, UnitKind>,
    candidates: Vec<UnitSummary>,
    disabled: bool,
    on_add: EventHandler<()>,
) -> Element {
    let inventory = form.read().initial_inventory.clone();

    rsx! {
        div { class: "flex flex-col gap-2 text-sm",
            span { class: "text-neutral-400", "Initial inventory" }
            div { class: "flex flex-wrap items-center gap-1.5",
                for kind in inventory.iter() {
                    {
                        let k = kind.clone();
                        let summary = candidates
                            .iter()
                            .find(|u| id_to_kind.get(&u.id) == Some(&k))
                            .cloned();
                        rsx! {
                            span { class: "inline-flex items-center gap-1.5 px-2 py-1 rounded bg-neutral-800 border border-neutral-700 text-xs text-neutral-200",
                                if let Some(ref u) = summary {
                                    img {
                                        src: "/api/portraits/{u.id}.png",
                                        alt: "{u.display_name}",
                                        class: "w-5 h-5 object-contain",
                                    }
                                }
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
                button {
                    class: "w-7 h-7 rounded bg-black border border-neutral-600 flex items-center justify-center text-neutral-500 hover:text-neutral-200 hover:border-neutral-400 transition-colors text-lg leading-none",
                    disabled: disabled,
                    title: "Add unit to inventory",
                    onclick: move |_| {
                        if !disabled {
                            on_add.call(());
                        }
                    },
                    "?"
                }
            }
        }
    }
}
