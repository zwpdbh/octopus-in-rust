use dioxus::prelude::*;
use faf_dioxus_ui::{ChartMetric, ChartSeries, ChartTab, RGBColor, UplotChart};
use faf_sim_protocol::EcoSnapshot;

/// Energy available for construction after paying maintenance.
pub fn energy_available(s: &EcoSnapshot) -> f64 {
    s.production_per_second_energy - s.maintenance_consumption_per_second_energy
}

/// Net energy change per second (income − maintenance − drain).
pub fn energy_net(s: &EcoSnapshot) -> f64 {
    energy_available(s) - s.energy_drain
}

/// FAF army-wide energy efficiency ratio used to scale mass income.
pub fn energy_efficiency(s: &EcoSnapshot) -> f64 {
    let requested = s.maintenance_consumption_per_second_energy + s.energy_drain;
    if requested <= 0.0 {
        1.0
    } else {
        (s.production_per_second_energy / requested).min(1.0)
    }
}

/// Mass income after applying FAF energy-stall scaling.
pub fn scaled_mass_income(s: &EcoSnapshot) -> f64 {
    if s.energy_storage < s.maintenance_consumption_per_second_energy {
        s.production_per_second_mass * energy_efficiency(s)
    } else {
        s.production_per_second_mass
    }
}

/// Net mass change per second (scaled income − drain).
pub fn mass_net(s: &EcoSnapshot) -> f64 {
    scaled_mass_income(s) - s.mass_drain
}

/// True when FAF would scale mass production because energy storage is below
/// total maintenance.
pub fn mass_scaling_active(s: &EcoSnapshot) -> bool {
    s.energy_storage < s.maintenance_consumption_per_second_energy
}

const ONE: fn(&EcoSnapshot) -> f64 = |_s: &EcoSnapshot| 1.0;

use crate::components::SnapshotDetails;

#[component]
pub fn EcoChart(data: Signal<Vec<EcoSnapshot>>, latest: Option<EcoSnapshot>) -> Element {
    let tabs = vec![
        ChartTab {
            label: "Energy budget".to_string(),
            series: vec![
                ChartSeries::new(
                    "Income",
                    RGBColor(34, 197, 94),
                    ChartMetric::new(|s: &EcoSnapshot| s.production_per_second_energy),
                ),
                ChartSeries::new(
                    "Maintenance",
                    RGBColor(234, 179, 8),
                    ChartMetric::new(|s: &EcoSnapshot| s.maintenance_consumption_per_second_energy),
                ),
                ChartSeries::new(
                    "Available",
                    RGBColor(59, 130, 246),
                    ChartMetric::new(energy_available),
                ),
                ChartSeries::new(
                    "Drain",
                    RGBColor(239, 68, 68),
                    ChartMetric::new(|s: &EcoSnapshot| s.energy_drain),
                ),
                ChartSeries::new("Net", RGBColor(168, 85, 247), ChartMetric::new(energy_net)),
            ],
        },
        ChartTab {
            label: "Mass budget".to_string(),
            series: vec![
                ChartSeries::new(
                    "Gross income",
                    RGBColor(156, 163, 175),
                    ChartMetric::new(|s: &EcoSnapshot| s.production_per_second_mass),
                )
                .with_dash([4.0, 4.0]),
                ChartSeries::new(
                    "Scaled income",
                    RGBColor(59, 130, 246),
                    ChartMetric::new(scaled_mass_income),
                ),
                ChartSeries::new(
                    "Drain",
                    RGBColor(239, 68, 68),
                    ChartMetric::new(|s: &EcoSnapshot| s.mass_drain),
                ),
                ChartSeries::new("Net", RGBColor(34, 197, 94), ChartMetric::new(mass_net)),
            ],
        },
        ChartTab {
            label: "Efficiency".to_string(),
            series: vec![
                ChartSeries::new(
                    "Energy efficiency",
                    RGBColor(59, 130, 246),
                    ChartMetric::new(energy_efficiency),
                ),
                ChartSeries::new("100%", RGBColor(156, 163, 175), ChartMetric::new(ONE))
                    .with_dash([2.0, 2.0]),
            ],
        },
        ChartTab {
            label: "Mass storage".to_string(),
            series: vec![
                ChartSeries::new(
                    "Current",
                    RGBColor(99, 102, 241),
                    ChartMetric::new(|s: &EcoSnapshot| s.mass_storage),
                ),
                ChartSeries::new(
                    "Cap",
                    RGBColor(168, 85, 247),
                    ChartMetric::new(|s: &EcoSnapshot| s.mass_storage_cap),
                ),
            ],
        },
        ChartTab {
            label: "Energy storage".to_string(),
            series: vec![
                ChartSeries::new(
                    "Current",
                    RGBColor(14, 165, 233),
                    ChartMetric::new(|s: &EcoSnapshot| s.energy_storage),
                ),
                ChartSeries::new(
                    "Cap",
                    RGBColor(236, 72, 153),
                    ChartMetric::new(|s: &EcoSnapshot| s.energy_storage_cap),
                ),
                ChartSeries::new(
                    "Maintenance threshold",
                    RGBColor(249, 115, 22),
                    ChartMetric::new(|s: &EcoSnapshot| s.maintenance_consumption_per_second_energy),
                )
                .with_dash([4.0, 4.0]),
            ],
        },
        ChartTab {
            label: "Mass spent".to_string(),
            series: vec![ChartSeries::new(
                "Total mass spent",
                RGBColor(34, 197, 94),
                ChartMetric::new(|s: &EcoSnapshot| s.total_mass_spent),
            )],
        },
        ChartTab {
            label: "Energy spent".to_string(),
            series: vec![ChartSeries::new(
                "Total energy spent",
                RGBColor(249, 115, 22),
                ChartMetric::new(|s: &EcoSnapshot| s.total_energy_spent),
            )],
        },
    ];

    let sidebar = latest.map(|snapshot| {
        rsx! {
            SnapshotDetails { snapshot }
        }
    });

    rsx! {
        div { class: "flex-1 min-h-0",
            UplotChart {
                data,
                x_extractor: ChartMetric::new(|s: &EcoSnapshot| s.time),
                tabs,
                sidebar,
            }
        }
    }
}
