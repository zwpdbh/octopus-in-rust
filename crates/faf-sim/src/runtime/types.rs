//! Public input/output types for the ECS economy runtime.
//!
//! Core queue/economy types live in `faf-sim-shared` so that the analytical
//! solver and other tooling can use them without depending on the Bevy ECS
//! runtime. This module re-exports those shared types and defines the
//! runtime-specific `SimulationEvent`.

use faf_quantities::Time;
use faf_sim_shared::EcoSnapshot;
use serde::{Deserialize, Serialize};

pub use faf_sim_shared::{BuildQueue, BuildTask};

/// A pending task together with the absolute simulation time at which it is
/// allowed to start. The scheduler updates `ready_at` when the preceding task
/// finishes so that `start_after` is interpreted relative to that finish time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScheduledTask {
    pub task: BuildTask,
    pub ready_at: Time,
}

impl ScheduledTask {
    pub fn new(task: BuildTask, ready_at: Time) -> Self {
        Self { task, ready_at }
    }
}

/// Observable event emitted by the simulation each step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimulationEvent {
    /// A tick happened and the economy is in the given state.
    // Ticked { time: f64, eco: GameEcoParameters },
    Ticking(EcoSnapshot),
    /// A task has become active.
    TaskStarted { task_id: u32, time: f64 },
    /// A task has finished.
    TaskCompleted { task_id: u32, time: f64 },
    /// The whole queue is done.
    Finished,
}
