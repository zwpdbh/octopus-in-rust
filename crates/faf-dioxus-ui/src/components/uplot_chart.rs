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
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::window;

const UPLOT_CSS: &str = "https://cdn.jsdelivr.net/npm/uplot@1.6.24/dist/uPlot.min.css";
const UPLOT_JS: &str = "https://cdn.jsdelivr.net/npm/uplot@1.6.24/dist/uPlot.iife.min.js";

use crate::components::line_chart_panel::ChartTab;
use crate::components::ChartMetric;

static CHART_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

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
    // Stores the active tab index and the live uPlot instance.
    let chart_state: Rc<RefCell<Option<(usize, JsValue)>>> =
        use_hook(|| Rc::new(RefCell::new(None)));
    let chart_state_for_effect = chart_state.clone();
    let chart_state_for_cleanup = chart_state.clone();

    use_drop(move || {
        if let Some((_, chart)) = chart_state_for_cleanup.borrow_mut().take() {
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
            Some((last_index, _)) => *last_index != tab_index,
            None => true,
        };

        if should_create {
            if let Some((_, old_chart)) = state.take() {
                let _ = destroy_chart(&old_chart);
            }
            match create_chart(
                &chart_id_for_effect,
                &points,
                x_extractor,
                tab.y_extractor,
                &tab.label,
                &rgb_to_hex(tab.color),
            ) {
                Some(chart) => {
                    *state = Some((tab_index, chart));
                }
                None => {
                    // uPlot may not be loaded yet. Retry shortly.
                    schedule_retry(load_attempts);
                }
            }
        } else if let Some((_, chart)) = state.as_ref() {
            let _ = update_chart(chart, &points, x_extractor, tab.y_extractor);
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
                        style: "background-color: {rgb_to_hex(active_tab.color)}",
                    }
                    h2 { class: "text-sm font-semibold text-white", "{active_tab.label}" }
                }
                div {
                    id: "{chart_id}",
                    class: "flex-1 min-h-0 uplot-chart-container",
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

fn create_chart<T>(
    element_id: &str,
    data: &[T],
    x_extractor: ChartMetric<T>,
    y_extractor: ChartMetric<T>,
    label: &str,
    color: &str,
) -> Option<JsValue> {
    let window = window()?;
    let document = window.document()?;
    let container = document.get_element_by_id(element_id)?;

    let series_data = build_series_data(data, x_extractor, y_extractor);
    let width = container.client_width().max(1) as f64;
    let height = container.client_height().max(1) as f64;
    let opts = build_opts(label, color, width, height);

    let uplot = Reflect::get(&window, &"uPlot".into()).ok()?;
    let uplot_fn = Function::from(uplot);
    let args = Array::new();
    args.push(&opts);
    args.push(&series_data);
    args.push(&container);
    Reflect::construct(&uplot_fn, &args).ok()
}

fn update_chart<T>(
    chart: &JsValue,
    data: &[T],
    x_extractor: ChartMetric<T>,
    y_extractor: ChartMetric<T>,
) -> Result<(), JsValue> {
    let series_data = build_series_data(data, x_extractor, y_extractor);
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

fn build_series_data<T>(
    data: &[T],
    x_extractor: ChartMetric<T>,
    y_extractor: ChartMetric<T>,
) -> Array {
    let xs = Array::new();
    let ys = Array::new();
    for point in data {
        xs.push(&JsValue::from_f64(x_extractor.extract(point)));
        ys.push(&JsValue::from_f64(y_extractor.extract(point)));
    }
    let series_data = Array::new();
    series_data.push(&xs);
    series_data.push(&ys);
    series_data
}

fn build_opts(label: &str, color: &str, width: f64, height: f64) -> Object {
    let opts = Object::new();
    Reflect::set(&opts, &"width".into(), &JsValue::from_f64(width)).unwrap();
    Reflect::set(&opts, &"height".into(), &JsValue::from_f64(height)).unwrap();

    let series = Array::new();
    series.push(&Object::new()); // x-axis series placeholder
    let y_series = Object::new();
    Reflect::set(&y_series, &"label".into(), &label.into()).unwrap();
    Reflect::set(&y_series, &"stroke".into(), &color.into()).unwrap();
    Reflect::set(&y_series, &"width".into(), &JsValue::from_f64(2.0)).unwrap();
    series.push(&y_series);
    Reflect::set(&opts, &"series".into(), &series).unwrap();

    let axes = Array::new();
    axes.push(&axis_opts());
    axes.push(&axis_opts());
    Reflect::set(&opts, &"axes".into(), &axes).unwrap();

    let scales = Object::new();
    let x_scale = Object::new();
    Reflect::set(&x_scale, &"auto".into(), &JsValue::from_bool(true)).unwrap();
    Reflect::set(&scales, &"x".into(), &x_scale).unwrap();
    let y_scale = Object::new();
    Reflect::set(&y_scale, &"auto".into(), &JsValue::from_bool(true)).unwrap();
    Reflect::set(&scales, &"y".into(), &y_scale).unwrap();
    Reflect::set(&opts, &"scales".into(), &scales).unwrap();

    opts
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
