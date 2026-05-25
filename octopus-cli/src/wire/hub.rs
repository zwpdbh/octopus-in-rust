use serde::Serialize;
use tokio::sync::broadcast;

use crate::wire::channel::WireSoulSide;

// ============================================================================
// Current wire soul side (per-run isolation)
// ============================================================================

thread_local! {
    static CURRENT_WIRE_SOUL_SIDE: std::cell::RefCell<Option<WireSoulSide>> = const { std::cell::RefCell::new(None) };
}

/// Set the current wire soul side for the duration of a soul run.
pub fn set_current_wire_soul_side(soul_side: Option<WireSoulSide>) {
    CURRENT_WIRE_SOUL_SIDE.with(|w| *w.borrow_mut() = soul_side);
}

/// Get the current wire soul side, if any.
pub fn get_current_wire_soul_side() -> Option<WireSoulSide> {
    CURRENT_WIRE_SOUL_SIDE.with(|w| w.borrow().clone())
}

// ============================================================================
// Root wire hub
// ============================================================================

#[derive(Clone, Debug)]
pub struct RootWireHub {
    tx: broadcast::Sender<serde_json::Value>,
}

impl RootWireHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.tx.subscribe()
    }

    pub fn publish(&self, msg: serde_json::Value) {
        let _ = self.tx.send(msg);
    }

    pub fn shutdown(&self) {
        // Dropping all clones closes receivers.
    }
}

impl Default for RootWireHub {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Wire send function
// ============================================================================

pub fn wire_send<T: Serialize>(event: T) {
    let value = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to serialize wire message: {}", e);
            return;
        }
    };

    if let Some(soul_side) = get_current_wire_soul_side() {
        soul_side.send(value);
    }
}
