use dioxus::prelude::*;
use faf_sim_protocol::EcoSnapshot;

use crate::components::eco_chart::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};
use crate::i18n::{self, Text};

/// Sidebar panel showing the latest economy snapshot details.
#[component]
pub fn SnapshotDetails(snapshot: EcoSnapshot) -> Element {
    let t = i18n::use_t();
    let scaled = scaled_mass_income(&snapshot);
    let mass_net_val = mass_net(&snapshot);
    let energy_avail = energy_available(&snapshot);
    let energy_net_val = energy_net(&snapshot);
    let efficiency = energy_efficiency(&snapshot);
    let scaling_active = mass_scaling_active(&snapshot);
    let scaling_label = if scaling_active {
        t.t(Text::ScalingActive)
    } else {
        ""
    };

    let (snapshot_l, time_l, production_l, scaled_l, drain_l, net_l, storage_l) = (
        t.t(Text::Snapshot),
        t.t(Text::Time),
        t.t(Text::Production),
        t.t(Text::Scaled),
        t.t(Text::Drain),
        t.t(Text::Net),
        t.t(Text::Storage),
    );
    let (maintenance_l, available_l, efficiency_l) = (
        t.t(Text::Maintenance),
        t.t(Text::Available),
        t.t(Text::Efficiency),
    );
    let (mass_l, energy_l) = (t.t(Text::MassCost), t.t(Text::EnergyCost));

    rsx! {
        div { class: "flex flex-col gap-2 w-56 shrink-0 self-start text-xs text-neutral-300",
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "{snapshot_l}" }
                div { "{time_l}: {snapshot.time:.1}s" }
            }
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "{mass_l}" }
                div { "{production_l}: {snapshot.production_per_second_mass:.2}" }
                div { "{scaled_l}: {scaled:.2}" }
                div { "{drain_l}: {snapshot.mass_drain:.2}" }
                div { "{net_l}: {mass_net_val:.2}" }
                div { "{storage_l}: {snapshot.mass_storage:.0} / {snapshot.mass_storage_cap:.0}" }
            }
            div { class: "p-2 rounded bg-neutral-900/80 border border-neutral-800",
                div { class: "font-semibold text-white mb-1", "{energy_l}" }
                div { "{production_l}: {snapshot.production_per_second_energy:.2}" }
                div { "{maintenance_l}: {snapshot.maintenance_consumption_per_second_energy:.2}" }
                div { "{available_l}: {energy_avail:.2}" }
                div { "{drain_l}: {snapshot.energy_drain:.2}" }
                div { "{net_l}: {energy_net_val:.2}" }
                div { "{storage_l}: {snapshot.energy_storage:.0} / {snapshot.energy_storage_cap:.0}" }
                div { "{efficiency_l}: {efficiency:.2}{scaling_label}" }
            }
        }
    }
}
