//! High-performance time-series chart using uPlot.
//!
//! This component renders a tab-switchable line chart backed by the uPlot
//! JavaScript library. It is designed for real-time streaming data: new points
//! are passed to uPlot via `setData`, which handles incremental rendering
//! internally.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dioxus::prelude::*;
use js_sys::{Array, Function, Object, Reflect};
use plotters::prelude::RGBColor;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::window;

const UPLOT_CSS: &str = "https://cdn.jsdelivr.net/npm/uplot@1.6.24/dist/uPlot.min.css";
const UPLOT_JS: &str = "https://cdn.jsdelivr.net/npm/uplot@1.6.24/dist/uPlot.iife.min.js";

static CHART_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

/// Configuration for one data series inside a chart tab.
#[derive(Clone, PartialEq)]
pub struct ChartSeries<T> {
    /// Label shown in the tooltip and legend.
    pub label: String,
    /// Color used for the line.
    pub color: RGBColor,
    /// Extracts the y value for a data point.
    pub y_extractor: ChartMetric<T>,
}

impl<T> ChartSeries<T> {
    pub fn new(label: impl Into<String>, color: RGBColor, y_extractor: ChartMetric<T>) -> Self {
        Self {
            label: label.into(),
            color,
            y_extractor,
        }
    }
}

/// Configuration for one tab/metric in the chart panel.
#[derive(Clone, PartialEq)]
pub struct ChartTab<T> {
    /// Label shown on the tab.
    pub label: String,
    /// Series plotted together on this tab.
    pub series: Vec<ChartSeries<T>>,
}

/// High-performance time-series chart backed by uPlot.
///
/// The component renders a single canvas. Tabs switch which metric is
/// displayed. Data updates are forwarded to uPlot through `setData`, so the
/// library can render new points efficiently without a full redraw from Rust.
#[component]
pub fn UplotChart<T: Clone + PartialEq + 'static>(
    data: Signal<Vec<T>>,
    x_extractor: ChartMetric<T>,
    tabs: Vec<ChartTab<T>>,
) -> Element {
    let mut selected_index = use_signal(|| 0usize);
    let load_attempts = use_signal(|| 0u32);
    let chart_id = use_hook(|| {
        let id = CHART_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        format!("uplot-chart-{id}")
    });
    // Stores the active tab index, the live uPlot instance, and any closures
    // installed as hooks so they stay alive as long as the chart.
    let chart_state: Rc<RefCell<Option<(usize, JsValue, Vec<Closure<dyn FnMut(JsValue)>>)>>> =
        use_hook(|| Rc::new(RefCell::new(None)));
    let chart_state_for_effect = chart_state.clone();
    let chart_state_for_cleanup = chart_state.clone();

    // Tooltip content and pixel position within the chart.
    // The tuple is (time_label, vec![(series_label, value_label)]).
    let tooltip = use_signal(|| None::<(String, Vec<(String, String)>)>);
    let tooltip_pos = use_signal(|| (0.0_f64, 0.0_f64));

    use_drop(move || {
        if let Some((_, chart, _)) = chart_state_for_cleanup.borrow_mut().take() {
            let _ = destroy_chart(&chart);
        }
    });

    let tabs_for_effect = tabs.clone();
    let chart_id_for_effect = chart_id.clone();
    use_effect(move || {
        let _ = *load_attempts.read();
        let points = data.read();
        let tab_index = *selected_index.read();
        let Some(tab) = tabs_for_effect.get(tab_index) else {
            return;
        };

        let mut state = chart_state_for_effect.borrow_mut();
        let should_create = match state.as_ref() {
            Some((last_index, _, _)) => *last_index != tab_index,
            None => true,
        };

        if should_create {
            if let Some((_, old_chart, _)) = state.take() {
                let _ = destroy_chart(&old_chart);
            }
            match create_chart(
                &chart_id_for_effect,
                data,
                x_extractor,
                &tab.series,
                tooltip,
                tooltip_pos,
            ) {
                Some((chart, hooks)) => {
                    *state = Some((tab_index, chart, hooks));
                }
                None => {
                    // uPlot may not be loaded yet. Retry shortly.
                    schedule_retry(load_attempts);
                }
            }
        } else if let Some((_, chart, _)) = state.as_ref() {
            let _ = update_chart(chart, &points, x_extractor, &tab.series);
        }
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
    let active_series = active_tab
        .series
        .first()
        .cloned()
        .unwrap_or_else(|| ChartSeries {
            label: active_tab.label.clone(),
            color: RGBColor(255, 255, 255),
            y_extractor: ChartMetric::new(|_| 0.0),
        });

    rsx! {
        document::Stylesheet { href: UPLOT_CSS }
        document::Script { src: UPLOT_JS }
        document::Style {
            r#"
            .uplot-chart-container .uplot {{
                width: 100% !important;
                height: 100% !important;
            }}
            .uplot-chart-container .uplot .title {{
                color: #ffffff;
            }}
            .uplot-chart-container .uplot .uplot-legend {{
                position: absolute;
                top: 0;
                right: 0;
                background-color: rgba(23, 23, 23, 0.9);
                border: 1px solid #404040;
                border-radius: 0.25rem;
                padding: 0.25rem 0.5rem;
                font-size: 0.65rem;
                color: #ffffff;
            }}
            .uplot-chart-container .uplot .uplot-legend th,
            .uplot-chart-container .uplot .uplot-legend td {{
                color: #ffffff;
            }}
            "#
        }
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
                        style: "background-color: {rgb_to_hex(active_series.color)}",
                    }
                    h2 { class: "text-sm font-semibold text-white", "{active_tab.label}" }
                }
                div { class: "flex-1 min-h-0 relative",
                    div {
                        id: "{chart_id}",
                        class: "absolute inset-0 uplot-chart-container",
                    }
                    if let Some((time, values)) = tooltip.read().as_ref() {
                        div {
                            class: "absolute z-10 px-2 py-1 rounded bg-neutral-900 border border-neutral-700 text-xs text-white shadow pointer-events-none",
                            style: "left: {tooltip_pos.read().0}px; top: {tooltip_pos.read().1 - 40.0}px;",
                            div { "{time}" }
                            for (label , value) in values.iter() {
                                div { "{label}: {value}" }
                            }
                        }
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

fn create_chart<T: Clone + 'static>(
    element_id: &str,
    data: Signal<Vec<T>>,
    x_extractor: ChartMetric<T>,
    series: &[ChartSeries<T>],
    tooltip: Signal<Option<(String, Vec<(String, String)>)>>,
    tooltip_pos: Signal<(f64, f64)>,
) -> Option<(JsValue, Vec<Closure<dyn FnMut(JsValue)>>)> {
    let window = window()?;
    let document = window.document()?;
    let container = document.get_element_by_id(element_id)?;

    let points = data.read();
    let series_data = build_series_data(&points, x_extractor, series);
    let width = container.client_width().max(1) as f64;
    let height = container.client_height().max(1) as f64;

    let set_cursor = build_set_cursor_closure(data, x_extractor, series, tooltip, tooltip_pos);
    let opts = build_opts(series, width, height, &set_cursor);

    let uplot = Reflect::get(&window, &"uPlot".into()).ok()?;
    let uplot_fn = Function::from(uplot);
    let args = Array::new();
    args.push(&opts);
    args.push(&series_data);
    args.push(&container);
    let chart = Reflect::construct(&uplot_fn, &args).ok()?;

    let hooks = vec![set_cursor];
    Some((chart, hooks))
}

fn update_chart<T: Clone>(
    chart: &JsValue,
    data: &[T],
    x_extractor: ChartMetric<T>,
    series: &[ChartSeries<T>],
) -> Result<(), JsValue> {
    let series_data = build_series_data(data, x_extractor, series);
    let set_data = Reflect::get(chart, &"setData".into())?;
    let set_data_fn = Function::from(set_data);
    Reflect::apply(&set_data_fn, chart, &Array::of1(&series_data))?;
    Ok(())
}

fn destroy_chart(chart: &JsValue) -> Result<(), JsValue> {
    let destroy = Reflect::get(chart, &"destroy".into())?;
    let destroy_fn = Function::from(destroy);
    Reflect::apply(&destroy_fn, chart, &Array::new())?;
    Ok(())
}

fn build_series_data<T: Clone>(
    data: &[T],
    x_extractor: ChartMetric<T>,
    series: &[ChartSeries<T>],
) -> Array {
    let xs = Array::new();
    let ys_list: Vec<Array> = series.iter().map(|_| Array::new()).collect();
    for point in data {
        xs.push(&JsValue::from_f64(x_extractor.extract(point)));
        for (i, s) in series.iter().enumerate() {
            ys_list[i].push(&JsValue::from_f64(s.y_extractor.extract(point)));
        }
    }
    let series_data = Array::new();
    series_data.push(&xs);
    for ys in ys_list {
        series_data.push(&ys);
    }
    series_data
}

fn build_opts<T: Clone>(
    series: &[ChartSeries<T>],
    width: f64,
    height: f64,
    set_cursor: &Closure<dyn FnMut(JsValue)>,
) -> Object {
    let opts = Object::new();
    Reflect::set(&opts, &"width".into(), &JsValue::from_f64(width)).unwrap();
    Reflect::set(&opts, &"height".into(), &JsValue::from_f64(height)).unwrap();

    let series_arr = Array::new();
    series_arr.push(&Object::new()); // x-axis series placeholder
    for s in series {
        let y_series = Object::new();
        Reflect::set(&y_series, &"label".into(), &s.label.as_str().into()).unwrap();
        Reflect::set(&y_series, &"stroke".into(), &rgb_to_hex(s.color).into()).unwrap();
        Reflect::set(&y_series, &"width".into(), &JsValue::from_f64(2.0)).unwrap();
        series_arr.push(&y_series);
    }
    Reflect::set(&opts, &"series".into(), &series_arr).unwrap();

    let axes = Array::new();
    axes.push(&x_axis_opts());
    axes.push(&axis_opts());
    Reflect::set(&opts, &"axes".into(), &axes).unwrap();

    let hooks = Object::new();
    let set_cursor_arr = Array::new();
    set_cursor_arr.push(set_cursor.as_ref());
    Reflect::set(&hooks, &"setCursor".into(), &set_cursor_arr).unwrap();
    Reflect::set(&opts, &"hooks".into(), &hooks).unwrap();

    let legend = Object::new();
    Reflect::set(&legend, &"show".into(), &JsValue::from_bool(true)).unwrap();
    Reflect::set(&opts, &"legend".into(), &legend).unwrap();

    let scales = Object::new();
    let x_scale = Object::new();
    Reflect::set(&x_scale, &"auto".into(), &JsValue::from_bool(true)).unwrap();
    Reflect::set(&x_scale, &"time".into(), &JsValue::from_bool(false)).unwrap();
    Reflect::set(&scales, &"x".into(), &x_scale).unwrap();
    let y_scale = Object::new();
    Reflect::set(&y_scale, &"auto".into(), &JsValue::from_bool(true)).unwrap();
    Reflect::set(&scales, &"y".into(), &y_scale).unwrap();
    Reflect::set(&opts, &"scales".into(), &scales).unwrap();

    opts
}

fn x_axis_opts() -> Object {
    let axis = axis_opts();
    Reflect::set(&axis, &"time".into(), &JsValue::from_bool(false)).unwrap();
    Reflect::set(&axis, &"label".into(), &"Time".into()).unwrap();

    let values_fn = Function::new_with_args(
        "_self, splits",
        r#"return splits.map(function(v) {
            if (v >= 60) return (v / 60).toFixed(1) + "m";
            return v.toFixed(0) + "s";
        });"#,
    );
    Reflect::set(&axis, &"values".into(), &values_fn).unwrap();

    axis
}

fn axis_opts() -> Object {
    let axis = Object::new();
    Reflect::set(&axis, &"stroke".into(), &"#a3a3a3".into()).unwrap();

    let grid = Object::new();
    Reflect::set(&grid, &"stroke".into(), &"#404040".into()).unwrap();
    Reflect::set(&grid, &"width".into(), &JsValue::from_f64(1.0)).unwrap();
    Reflect::set(&axis, &"grid".into(), &grid).unwrap();

    let ticks = Object::new();
    Reflect::set(&ticks, &"stroke".into(), &"#525252".into()).unwrap();
    Reflect::set(&ticks, &"width".into(), &JsValue::from_f64(1.0)).unwrap();
    Reflect::set(&axis, &"ticks".into(), &ticks).unwrap();

    axis
}

fn build_set_cursor_closure<T: Clone + 'static>(
    data: Signal<Vec<T>>,
    x_extractor: ChartMetric<T>,
    series: &[ChartSeries<T>],
    mut tooltip: Signal<Option<(String, Vec<(String, String)>)>>,
    mut tooltip_pos: Signal<(f64, f64)>,
) -> Closure<dyn FnMut(JsValue)> {
    let series = series.to_vec();
    Closure::wrap(Box::new(move |u: JsValue| {
        let Ok(cursor) = Reflect::get(&u, &"cursor".into()) else {
            tooltip.set(None);
            return;
        };
        let Ok(idx_val) = Reflect::get(&cursor, &"idx".into()) else {
            tooltip.set(None);
            return;
        };
        let Some(idx) = idx_val.as_f64() else {
            tooltip.set(None);
            return;
        };
        if idx < 0.0 {
            tooltip.set(None);
            return;
        }

        let idx = idx as usize;
        let points = data.read();
        let Some(point) = points.get(idx) else {
            tooltip.set(None);
            return;
        };

        let x = x_extractor.extract(point);
        let values = series
            .iter()
            .map(|s| {
                let y = s.y_extractor.extract(point);
                (s.label.clone(), format!("{:.2}", y))
            })
            .collect();
        tooltip.set(Some((format_time(x), values)));

        if let (Some(left), Some(top)) = (
            Reflect::get(&cursor, &"left".into())
                .ok()
                .and_then(|v| v.as_f64()),
            Reflect::get(&cursor, &"top".into())
                .ok()
                .and_then(|v| v.as_f64()),
        ) {
            tooltip_pos.set((left, top));
        }
    }) as Box<dyn FnMut(JsValue)>)
}

fn format_time(seconds: f64) -> String {
    if seconds >= 60.0 {
        format!("{:.1}m", seconds / 60.0)
    } else {
        format!("{:.1}s", seconds)
    }
}

fn rgb_to_hex(color: plotters::prelude::RGBColor) -> String {
    format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
}

fn schedule_retry(mut load_attempts: Signal<u32>) {
    let Some(window) = window() else {
        return;
    };
    let closure = Closure::wrap(Box::new(move || {
        let next = load_attempts.read().wrapping_add(1);
        load_attempts.set(next);
    }) as Box<dyn FnMut()>);
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        50,
    );
    closure.forget();
}
