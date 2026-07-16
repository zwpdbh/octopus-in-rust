use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use js_sys::Reflect;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{console, CustomEvent, Event};

use crate::types::{G6GraphData, UnitSummary};

type ClickListener = Rc<RefCell<Option<Closure<dyn FnMut(Event)>>>>;

/// Render an interactive blueprint dependency graph with AntV G6.
///
/// The component mounts G6 into `#g6-container`, listens for
/// `faf:g6-node-click` custom events, and forwards the clicked unit summary.
#[component]
pub fn BlueprintGraphG6(data: G6GraphData, on_node_click: EventHandler<UnitSummary>) -> Element {
    let listener: ClickListener = use_hook(|| Rc::new(RefCell::new(None)));
    let listener_for_effect = listener.clone();
    let listener_for_drop = listener.clone();

    use_effect(move || {
        // Attach the custom-event listener only once.
        if listener_for_effect.borrow().is_none() {
            let data = data.clone();
            let on_click = on_node_click;
            let closure = Closure::wrap(Box::new(move |event: Event| {
                if let Some(custom) = event.dyn_ref::<CustomEvent>() {
                    if let Some(detail) = custom.detail().as_string() {
                        if let Some(node) = data.nodes.iter().find(|n| n.id == detail) {
                            on_click.call(node.summary.clone());
                        }
                    }
                }
            }) as Box<dyn FnMut(_)>);

            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                let _ = document.add_event_listener_with_callback(
                    "faf:g6-node-click",
                    closure.as_ref().unchecked_ref(),
                );
            }
            *listener_for_effect.borrow_mut() = Some(closure);
        }

        // (Re-)initialize the graph whenever this effect runs.
        if let Ok(json) = serde_json::to_string(&data) {
            console::log_1(&"[BlueprintGraphG6] initializing G6".into());
            if let Err(err) = init_g6("g6-container", &json) {
                console::error_1(&format!("[BlueprintGraphG6] init failed: {err:?}").into());
            }
        }
    });

    use_drop(move || {
        if let Some(closure) = listener_for_drop.borrow_mut().take() {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                let _ = document.remove_event_listener_with_callback(
                    "faf:g6-node-click",
                    closure.as_ref().unchecked_ref(),
                );
            }
        }
        destroy_g6();
    });

    rsx! {
        div {
            id: "g6-container",
            class: "w-full",
            style: "height: 600px;",
        }
    }
}

fn init_g6(container_id: &str, json: &str) -> Result<(), ()> {
    let window = web_sys::window().ok_or(())?;
    let faf_g6 = Reflect::get(&window, &"fafG6".into()).map_err(|_| ())?;
    let init = Reflect::get(&faf_g6, &"init".into()).map_err(|_| ())?;
    let init_fn = init.dyn_into::<js_sys::Function>().map_err(|_| ())?;
    init_fn
        .call2(&faf_g6, &container_id.into(), &json.into())
        .map_err(|_| ())?;
    Ok(())
}

fn destroy_g6() {
    if let Some(window) = web_sys::window() {
        if let Ok(faf_g6) = Reflect::get(&window, &"fafG6".into()) {
            if let Ok(destroy) = Reflect::get(&faf_g6, &"destroy".into()) {
                if let Ok(destroy_fn) = destroy.dyn_into::<js_sys::Function>() {
                    let _ = destroy_fn.call0(&faf_g6);
                }
            }
        }
    }
}
