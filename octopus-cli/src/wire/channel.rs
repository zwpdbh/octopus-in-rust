use serde_json::Value;
use tokio::sync::broadcast;

use crate::wire::file::WireFile;

/// A spmc channel for communication between the soul and the UI during a soul run.
pub struct Wire {
    raw_tx: broadcast::Sender<Value>,
    merged_tx: broadcast::Sender<Value>,
    _recorder_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Wire {
    pub fn new(file_backend: Option<WireFile>) -> Self {
        let (raw_tx, _) = broadcast::channel(256);
        let (merged_tx, _merged_rx) = broadcast::channel(256);

        let recorder_handle = file_backend.map(|file| {
            let mut rx = merged_tx.subscribe();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(msg) => {
                            if let Err(e) = file.append_message(&msg).await {
                                tracing::warn!("Wire recorder failed: {}", e);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            tracing::debug!("Wire recorder lagged behind");
                            continue;
                        }
                    }
                }
            })
        });

        Self {
            raw_tx,
            merged_tx,
            _recorder_handle: recorder_handle,
        }
    }

    pub fn soul_side(&self) -> WireSoulSide {
        WireSoulSide {
            raw_tx: self.raw_tx.clone(),
            merged_tx: self.merged_tx.clone(),
        }
    }

    pub fn ui_side(&self) -> WireUISide {
        WireUISide {
            raw_rx: self.raw_tx.subscribe(),
        }
    }

    /// Shutdown the wire by dropping all senders, which causes receivers to close.
    pub fn shutdown(self) {
        // Dropping self drops the original senders.
        // Any cloned senders (e.g. in WireSoulSide) must also be dropped separately.
    }
}

/// The soul side of a `Wire`.
#[derive(Clone)]
pub struct WireSoulSide {
    raw_tx: broadcast::Sender<Value>,
    merged_tx: broadcast::Sender<Value>,
}

impl WireSoulSide {
    /// Send a message to the wire. Non-blocking.
    pub fn send(&self, msg: Value) {
        let _ = self.raw_tx.send(msg.clone());
        let _ = self.merged_tx.send(msg);
    }
}

/// The UI side of a `Wire`.
pub struct WireUISide {
    raw_rx: broadcast::Receiver<Value>,
}

impl WireUISide {
    pub async fn recv(&mut self) -> Result<Value, broadcast::error::RecvError> {
        self.raw_rx.recv().await
    }
}
