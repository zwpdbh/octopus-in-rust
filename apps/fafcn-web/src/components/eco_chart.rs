use dioxus::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
use faf_dioxus_ui::{ChartMetric, ChartSeries, ChartTab, RGBColor, UplotChart};

/// A single point on the economy time-series chart.
#[derive(Clone, PartialEq)]
pub struct EcoPoint {
    pub time: f64,
    pub mass_in_storage: f64,
    pub energy_in_storage: f64,
    pub mass_generate_rate: f64,
    pub energy_generate_rate: f64,
    pub mass_drain: f64,
    pub energy_drain: f64,
}

impl EcoPoint {
    pub fn new(time: f64, eco: &PlayerEcoMetrics) -> Self {
        Self {
            time,
            mass_in_storage: eco.mass_in_storage,
            energy_in_storage: eco.energy_in_storage,
            mass_generate_rate: eco.mass_generate_rate,
            energy_generate_rate: eco.energy_generate_rate,
            mass_drain: eco.mass_drain,
            energy_drain: eco.energy_drain,
        }
    }
}

#[component]
pub fn EcoChart(data: Signal<Vec<EcoPoint>>) -> Element {
    let tabs = vec![
        ChartTab {
            label: "Storage".to_string(),
            series: vec![
                ChartSeries::new(
                    "Mass",
                    RGBColor(34, 197, 94),
                    ChartMetric::new(|p: &EcoPoint| p.mass_in_storage),
                ),
                ChartSeries::new(
                    "Energy",
                    RGBColor(250, 204, 21),
                    ChartMetric::new(|p: &EcoPoint| p.energy_in_storage),
                ),
            ],
        },
        ChartTab {
            label: "Income".to_string(),
            series: vec![
                ChartSeries::new(
                    "Mass Income",
                    RGBColor(34, 197, 94),
                    ChartMetric::new(|p: &EcoPoint| p.mass_generate_rate),
                ),
                ChartSeries::new(
                    "Energy Income",
                    RGBColor(250, 204, 21),
                    ChartMetric::new(|p: &EcoPoint| p.energy_generate_rate),
                ),
            ],
        },
        ChartTab {
            label: "Drain".to_string(),
            series: vec![
                ChartSeries::new(
                    "Mass Drain",
                    RGBColor(239, 68, 68),
                    ChartMetric::new(|p: &EcoPoint| p.mass_drain),
                ),
                ChartSeries::new(
                    "Energy Drain",
                    RGBColor(249, 115, 22),
                    ChartMetric::new(|p: &EcoPoint| p.energy_drain),
                ),
            ],
        },
    ];

    rsx! {
        div { class: "flex-1 min-h-0",
            UplotChart {
                data,
                x_extractor: ChartMetric::new(|p: &EcoPoint| p.time),
                tabs,
            }
        }
    }
}
