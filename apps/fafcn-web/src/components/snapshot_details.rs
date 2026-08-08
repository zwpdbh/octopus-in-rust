use dioxus::prelude::*;
use faf_sim_protocol::EcoSnapshot;

use crate::components::eco_chart::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};

/// Sidebar panel showing the latest economy snapshot details.
#[component]
pub fn SnapshotDetails(snapshot: EcoSnapshot) -> Element {
    let scaled = scaled_mass_income(&snapshot);
    let mass_net_val = mass_net(&snapshot);
    let energy_avail = energy_available(&snapshot);
    let energy_net_val = energy_net(&snapshot);
    let efficiency = energy_efficiency(&snapshot);
    let scaling_active = mass_scaling_active(&snapshot);
    let scaling_label = if scaling_active {
        " (scaling active)"
    } else {
        ""
    };

    rsx! {
        div { class: "flex flex-col gap-2 w-56 shrink-0 self-start text-xs text-neutral-300",
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "Snapshot" }
                div { "Time: {snapshot.time:.1}s" }
            }
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "Mass" }
                div { "Production: {snapshot.production_per_second_mass:.2}" }
                div { "Scaled: {scaled:.2}" }
                div { "Drain: {snapshot.mass_drain:.2}" }
                div { "Net: {mass_net_val:.2}" }
                div { "Storage: {snapshot.mass_storage:.0} / {snapshot.mass_storage_cap:.0}" }
            }
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "Energy" }
                div { "Production: {snapshot.production_per_second_energy:.2}" }
                div { "Maintenance: {snapshot.maintenance_consumption_per_second_energy:.2}" }
                div { "Available: {energy_avail:.2}" }
                div { "Drain: {snapshot.energy_drain:.2}" }
                div { "Net: {energy_net_val:.2}" }
                div { "Storage: {snapshot.energy_storage:.0} / {snapshot.energy_storage_cap:.0}" }
                div { "Efficiency: {efficiency:.2}{scaling_label}" }
            }
        }
    }
}
