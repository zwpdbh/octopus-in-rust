use dioxus::prelude::*;
use faf_blueprints::ConstructionPlan;
use faf_sim_protocol::{SimClientMessage, SimEvent, SimServerMessage, SimSpeed};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

/// Connection handle to the simulation server.
#[derive(Clone)]
pub struct SimConnection {
    ws: WebSocket,
}

impl SimConnection {
    /// Open a WebSocket to the simulation server and wire it into Dioxus signals.
    ///
    /// `on_event` is called for every `SimEvent`.  `on_status` is called when
    /// the connection opens/closes/errors or the simulation finishes.
    pub fn open(
        plan: ConstructionPlan,
        speed: SimSpeed,
        on_event: impl FnMut(SimEvent) + 'static,
        on_status: impl FnMut(String) + 'static,
    ) -> Result<Self, JsValue> {
        let url = crate::net::ws_url("/ws/simulate");

        let ws = WebSocket::new(&url)?;

        // Wrap callbacks so multiple closures can share them.
        let on_event = Rc::new(RefCell::new(on_event));
        let on_status = Rc::new(RefCell::new(on_status));

        let start_text =
            serde_json::to_string(&SimClientMessage::StartPlan { plan, speed }).unwrap_or_default();

        let status = on_status.clone();
        let onopen = Closure::wrap(Box::new(move |e: Event| {
            if let Ok(socket) = e.target().unwrap().dyn_into::<WebSocket>() {
                let _ = socket.send_with_str(&start_text);
            }
            (status.borrow_mut())("connected".to_string());
        }) as Box<dyn FnMut(_)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let status = on_status.clone();
        let onerror = Closure::wrap(Box::new(move |_: Event| {
            (status.borrow_mut())("error".to_string());
        }) as Box<dyn FnMut(_)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let status = on_status.clone();
        let onclose = Closure::wrap(Box::new(move |_: CloseEvent| {
            (status.borrow_mut())("finished".to_string());
        }) as Box<dyn FnMut(_)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        let status = on_status.clone();
        let event = on_event.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<SimServerMessage>(&text) {
                    Ok(SimServerMessage::Event(ev)) => (event.borrow_mut())(ev),
                    Ok(SimServerMessage::Finished) => (status.borrow_mut())("finished".to_string()),
                    Ok(SimServerMessage::Error(err)) => {
                        (status.borrow_mut())(format!("error: {err}"))
                    }
                    Err(err) => (status.borrow_mut())(format!("parse error: {err}")),
                }
            }
        }) as Box<dyn FnMut(_)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        Ok(Self { ws })
    }

    pub fn send_command(&self, cmd: faf_sim_protocol::SimCmd) {
        let msg = SimClientMessage::Command(cmd);
        let _ = self
            .ws
            .send_with_str(&serde_json::to_string(&msg).unwrap_or_default());
    }

    pub fn close(&self) {
        let _ = self.ws.close();
    }
}

/// Hook that creates and manages a simulation connection.
pub fn use_sim_connection() -> Signal<Option<SimConnection>> {
    use_signal(|| None::<SimConnection>)
}
