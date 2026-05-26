use tokio::sync::broadcast;

use crate::wire::WireEvent;
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
    tx: broadcast::Sender<WireEvent>,
}

impl RootWireHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WireEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, msg: WireEvent) {
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

pub fn wire_send(event: WireEvent) {
    if let Some(soul_side) = get_current_wire_soul_side() {
        soul_side.send(event);
    }
}
