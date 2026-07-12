//! Real-time line chart panel with tab-switchable metrics.
//!
//! The panel owns a single canvas. Callers provide a [`Signal<Vec<T>>`] data
//! source, an x-axis extractor, and a list of [`ChartTab`] configurations.
//! Clicking a tab redraws the chart for the selected metric; new data points
//! also trigger a redraw of the currently selected metric.

use dioxus::prelude::*;
use plotters::prelude::*;
use plotters_canvas::CanvasBackend;

const LINE_CHART_CANVAS: &str = "line-chart-canvas";

/// Wrapper around a function pointer so it can be used as a Dioxus prop
/// without triggering function-pointer equality warnings.
pub struct ChartMetric<T>(fn(&T) -> f64);

impl<T> ChartMetric<T> {
    pub fn new(extractor: fn(&T) -> f64) -> Self {
        Self(extractor)
    }

    pub(crate) fn extract(&self, value: &T) -> f64 {
        (self.0)(value)
    }
}

impl<T> Clone for ChartMetric<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ChartMetric<T> {}

impl<T> PartialEq for ChartMetric<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Configuration for one tab/metric in the chart panel.
#[derive(Clone, PartialEq)]
pub struct ChartTab<T> {
    /// Label shown on the tab and above the chart.
    pub label: String,
    /// Color used for the line and the indicator dot.
    pub color: RGBColor,
    /// Extracts the y value for a data point.
    pub y_extractor: ChartMetric<T>,
}

/// Real-time line chart with tab-switchable metrics.
#[component]
pub fn LineChartPanel<T: Clone + PartialEq + 'static>(
    data: Signal<Vec<T>>,
    x_extractor: ChartMetric<T>,
    tabs: Vec<ChartTab<T>>,
) -> Element {
    let mut selected_index = use_signal(|| 0usize);

    let tabs_for_effect = tabs.clone();
    use_effect(move || {
        let points = data.read();
        let index = *selected_index.read();
        let Some(tab) = tabs_for_effect.get(index) else {
            return;
        };
        if points.len() < 2 {
            return;
        }
        draw_line_chart(
            LINE_CHART_CANVAS,
            &points,
            x_extractor,
            tab.y_extractor,
            tab.color,
        );
    });

    if tabs.is_empty() {
        return rsx! {
            div { class: "flex-1 flex items-center justify-center",
                p { class: "text-sm text-neutral-500", "No chart tabs configured." }
            }
        };
    }

    let active_tab = tabs
        .get(*selected_index.read())
        .cloned()
        .unwrap_or_else(|| tabs[0].clone());
    let color_css = format!(
        "rgb({}, {}, {})",
        active_tab.color.0, active_tab.color.1, active_tab.color.2
    );

    rsx! {
        div { class: "flex-1 flex flex-col min-h-0",
            div { class: "flex gap-1 mb-2 shrink-0 flex-wrap",
                for (index , tab) in tabs.iter().enumerate() {
                    TabButton {
                        label: tab.label.clone(),
                        is_active: *selected_index.read() == index,
                        onclick: move |_| selected_index.set(index),
                    }
                }
            }
            div { class: "flex-1 rounded-lg border border-neutral-800 bg-[#171717] p-2 min-h-0 overflow-hidden flex flex-col",
                div { class: "flex items-center gap-2 mb-1 shrink-0",
                    div {
                        class: "w-3 h-3 rounded-full",
                        style: "background-color: {color_css}",
                    }
                    h2 { class: "text-sm font-semibold text-white", "{active_tab.label}" }
                }
                div { class: "flex-1 min-h-0 flex items-center justify-center",
                    canvas {
                        id: "{LINE_CHART_CANVAS}",
                        width: "800",
                        height: "480",
                        class: "w-full h-auto max-h-full rounded border border-neutral-800",
                    }
                }
            }
        }
    }
}

#[component]
fn TabButton(label: String, is_active: bool, onclick: EventHandler<()>) -> Element {
    let base = "px-3 py-1.5 text-sm rounded transition-colors";
    let active_class = if is_active {
        "bg-blue-700 text-white"
    } else {
        "bg-neutral-800 text-neutral-300 hover:bg-neutral-700 border border-neutral-700"
    };
    rsx! {
        button {
            class: "{base} {active_class}",
            onclick: move |_| onclick.call(()),
            "{label}"
        }
    }
}

fn draw_line_chart<T>(
    canvas_id: &str,
    data: &[T],
    x_extractor: ChartMetric<T>,
    y_extractor: ChartMetric<T>,
    color: RGBColor,
) {
    let backend = match CanvasBackend::new(canvas_id) {
        Some(b) => b,
        None => return,
    };
    let root = backend.into_drawing_area();
    root.fill(&RGBColor(23, 23, 23)).unwrap();

    if data.len() < 2 {
        root.present().unwrap();
        return;
    }

    let full_data: Vec<(f64, f64)> = data
        .iter()
        .map(|d| (x_extractor.extract(d), y_extractor.extract(d)))
        .collect();
    let max_x = data
        .last()
        .map(|d| x_extractor.extract(d))
        .unwrap_or(1.0)
        .max(1.0);
    let (min_y, max_y) = range_for_series(&full_data);

    let mut chart = ChartBuilder::on(&root)
        .margin(8)
        .x_label_area_size(0)
        .y_label_area_size(0)
        .build_cartesian_2d(0.0..max_x, min_y..max_y)
        .unwrap();

    chart
        .configure_mesh()
        .x_labels(0)
        .y_labels(0)
        .light_line_style(RGBColor(60, 60, 60))
        .draw()
        .unwrap();

    chart
        .draw_series(LineSeries::new(full_data, &color))
        .unwrap();

    root.present().unwrap();
}

fn range_for_series(data: &[(f64, f64)]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for (_, y) in data {
        min = min.min(*y);
        max = max.max(*y);
    }
    if min.is_infinite() || max.is_infinite() || min == max {
        return (0.0, 1.0);
    }
    let padding = (max - min) * 0.05;
    let min = (min - padding).max(0.0);
    let max = max + padding;
    if max <= min {
        return (min, min + 1.0);
    }
    (min, max)
}
