//! WebSocket connection to the training server (`/ws/training`), modeled on
//! fafcn-web's `websocket_service.rs`.

use std::{cell::RefCell, rc::Rc};

use faf_ml_core::{TrainingClientMessage, TrainingCommand, TrainingConfig, TrainingServerMessage};
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

/// Connection handle to the training server.
#[derive(Clone)]
pub struct TrainingConnection {
    ws: WebSocket,
}

impl TrainingConnection {
    /// Open a WebSocket to the training server and wire it into Dioxus signals.
    ///
    /// `on_message` is called for every `TrainingServerMessage`. `on_status`
    /// is called when the connection opens/closes/errors or the run finishes.
    /// Callbacks are intentionally `.forget()`ed — one connection per page,
    /// replaced when a new run starts (same trade-off as fafcn's
    /// `SimConnection`).
    /// Open a WebSocket and START a new training run.
    pub fn open(
        config: TrainingConfig,
        speed: f64,
        on_message: impl FnMut(TrainingServerMessage) + 'static,
        on_status: impl FnMut(String) + 'static,
    ) -> Result<Self, JsValue> {
        let start_text = serde_json::to_string(&TrainingClientMessage::Start { config, speed })
            .unwrap_or_default();
        Self::connect(start_text, on_message, on_status)
    }

    /// Open a WebSocket and ATTACH to the currently active run (replay +
    /// live stream; starts nothing). Used when the page loads mid-run.
    pub fn open_attach(
        on_message: impl FnMut(TrainingServerMessage) + 'static,
        on_status: impl FnMut(String) + 'static,
    ) -> Result<Self, JsValue> {
        Self::connect(
            serde_json::to_string(&TrainingClientMessage::Attach).unwrap_or_default(),
            on_message,
            on_status,
        )
    }

    fn connect(
        start_text: String,
        on_message: impl FnMut(TrainingServerMessage) + 'static,
        on_status: impl FnMut(String) + 'static,
    ) -> Result<Self, JsValue> {
        let url = crate::net::ws_url("/ws/training");
        let ws = WebSocket::new(&url)?;

        // Wrap callbacks so multiple closures can share them.
        let on_message = Rc::new(RefCell::new(on_message));
        let on_status = Rc::new(RefCell::new(on_status));

        let status = on_status.clone();
        let onopen = Closure::wrap(Box::new(move |e: Event| {
            if let Some(socket) = e.target().and_then(|t| t.dyn_into::<WebSocket>().ok()) {
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

        let message = on_message.clone();
        let status = on_status.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                match serde_json::from_str::<TrainingServerMessage>(&text) {
                    Ok(
                        msg @ (TrainingServerMessage::Metrics(_)
                        | TrainingServerMessage::Status(_)
                        | TrainingServerMessage::Reset),
                    ) => (message.borrow_mut())(msg),
                    Ok(TrainingServerMessage::Finished) => {
                        (status.borrow_mut())("finished".to_string())
                    }
                    Ok(TrainingServerMessage::Error(err)) => {
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

    pub fn send_command(&self, cmd: TrainingCommand) {
        let msg = TrainingClientMessage::Command(cmd);
        let _ = self
            .ws
            .send_with_str(&serde_json::to_string(&msg).unwrap_or_default());
    }
}
