//! WebSocket protocol shared between the FAF simulation frontend and backend.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::runtime::{BuildQueue, SimulationEvent};

/// Identifier for a running simulation.
pub type SimulationId = Uuid;

/// How a client wants the simulation to be driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimulationMode {
    /// The simulation only steps when the client sends `Advance`.
    Active,
    /// The simulation auto-steps and streams snapshots in real time.
    Passive { tick_interval_ms: u64 },
}

/// Message sent by the client (frontend or CLI) to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimClientMessage {
    /// Start a new simulation.
    Start {
        /// Build queue to simulate.
        queue: BuildQueue,
        /// Simulation step size in seconds. Must be an integer >= 1.
        dt_seconds: u32,
        /// Optional hard cap in seconds. When `None` the simulation runs until
        /// the build queue is empty.
        max_time_seconds: Option<u32>,
        /// How the simulation should be driven.
        mode: SimulationMode,
    },
    /// Subscribe to an existing simulation.
    Subscribe { simulation_id: SimulationId },
    /// Pause a running simulation.
    Pause { simulation_id: SimulationId },
    /// Resume a paused simulation.
    Resume { simulation_id: SimulationId },
    /// Stop a running simulation.
    Stop { simulation_id: SimulationId },
    /// Advance a simulation by one manual step of `dt_seconds` simulation seconds.
    Advance {
        simulation_id: SimulationId,
        dt_seconds: u32,
    },
}

/// Runtime state of a simulation as exposed by the service.
///
/// This state describes how the service thread is currently driving the
/// simulation, *not* the internal state of the [`Simulation`](crate::sim::Simulation)
/// model. `Simulation` is a synchronous stepper: it has no "running" or "paused"
/// concept of its own; it simply advances one tick every time [`Simulation::step`]
/// is called and reports whether the build queue is exhausted via
/// [`Simulation::is_finished`].
///
/// Keeping `SimRuntimeStatus` out of `Simulation` preserves that separation:
///
/// * `Simulation` is a pure, deterministic model. Its only stateful concern is
///   the build queue and the economy clock.
/// * `SimRuntimeStatus` is an orchestration concern owned by the service thread
///   (`RunState` in `faf-sim-service`). It answers "should the thread be
///   auto-stepping, waiting, or shutting down?"
/// * Pausing therefore does not mutate the model; it only stops the driver from
///   calling `step()`. This makes the model easier to test and reason about,
///   because a paused simulation and a running simulation produce identical
///   results when stepped the same number of times.
///
/// Clients observe state transitions through [`ControlEvent::StateChanged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimRuntimeStatus {
    /// The service thread is auto-stepping the simulation.
    Running,
    /// The service thread is waiting and will not step until it receives
    /// `Resume` or `Advance`.
    Paused,
    /// The service thread has exited and the simulation is no longer running.
    Stopped,
}

/// Event produced by a control command, as opposed to a simulation step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlEvent {
    /// The simulation transitioned from one runtime state to another.
    StateChanged {
        from: SimRuntimeStatus,
        to: SimRuntimeStatus,
    },
}

/// Message sent by the server to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SimServerMessage {
    /// Acknowledges that a simulation has started and provides its ID.
    Started { simulation_id: SimulationId },
    /// One simulation event produced by a step.
    Event(SimulationEvent),
    /// One control event produced by a command.
    ControlEvent(ControlEvent),
    /// Error that aborts the simulation or the client session.
    Error { message: String },
}
