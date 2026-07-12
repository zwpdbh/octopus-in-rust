//! WebSocket protocol shared between the FAF simulation frontend and backend.

use serde::{Deserialize, Serialize};

pub use crate::eco::{BuildQueue, SimulationEvent};

/// Message sent by the client (frontend) to the server to control simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimClientMessage {
    /// Start a new simulation.
    Start {
        /// Build queue to simulate.
        queue: BuildQueue,
        /// Simulation resolution in steps per second.
        resolution: u32,
        /// Optional hard cap in seconds. When `None` the simulation runs until
        /// the build queue is empty.
        max_time: Option<f64>,
    },
}

/// Message sent by the server (backend) to the client with simulation updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimServerMessage {
    /// One simulation event produced by a step.
    Event(SimulationEvent),
    /// Error that aborts the simulation.
    Error { message: String },
}
