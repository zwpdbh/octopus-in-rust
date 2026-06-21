use tokio::sync::broadcast;

use crate::wire::WireEvent;
use crate::wire::channel::WireSoulSide;

// ============================================================================
// Current wire soul side (per-run isolation)
// ============================================================================

tokio::task_local! {
    static CURRENT_WIRE_SOUL_SIDE: Option<WireSoulSide>;
}

/// Get the current wire soul side, if any.
pub fn get_current_wire_soul_side() -> Option<WireSoulSide> {
    CURRENT_WIRE_SOUL_SIDE
        .try_with(|w| w.clone())
        .unwrap_or(None)
}

/// Run a future with the given wire soul side set as the current side.
pub async fn with_wire_soul_side<F, T>(side: Option<WireSoulSide>, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    CURRENT_WIRE_SOUL_SIDE.scope(side, f).await
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
