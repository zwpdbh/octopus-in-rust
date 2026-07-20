//! Public input/output types for the ECS economy runtime.
//!
//! These types are deliberately lightweight and serializable so that callers
//! (the Dioxus web app, the CLI, tests, and the WebSocket service) can describe
//! units and queues without depending on the full `Units` repository.

use serde::{Deserialize, Serialize};

use crate::economy::EconomyRuntimeState;
use crate::quantities::Time;
use faf_blueprints::UnitEcoStats;

/// One task in a build queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildTask {
    /// Caller-defined id, echoed back in start/complete events.
    pub id: u32,
    /// Delay after the previous task finishes before this task may begin.
    ///
    /// For the first task this is a delay relative to simulation start (time 0).
    #[serde(default = "default_start_after")]
    pub start_after: Time,

    /// Builders assigned to the task.
    pub builders: Vec<UnitEcoStats>,
    /// Units being built, in order. Builders work through the list sequentially.
    pub targets: Vec<UnitEcoStats>,
}

fn default_start_after() -> Time {
    Time::from_raw(1.0)
}

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

/// A full build queue to simulate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildQueue {
    /// Initial economy state (income and storage).
    pub initial_eco: EconomyRuntimeState,
    /// Tasks to run, in queue order.
    pub tasks: Vec<BuildTask>,
}

/// A point-in-time view of the economy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoSnapshot {
    pub time: f64,
    /// Gross FAF `ProductionPerSecondMass`.
    pub production_per_second_mass: f64,
    /// Gross FAF `ProductionPerSecondEnergy`.
    pub production_per_second_energy: f64,
    /// Total FAF `MaintenanceConsumptionPerSecondEnergy` paid by all owned units.
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: f64,
    /// Total mass requested by all active construction sites per second.
    #[serde(default)]
    pub mass_drain: f64,
    /// Total energy requested by all active construction sites per second.
    #[serde(default)]
    pub energy_drain: f64,
    pub total_mass_spent: f64,
    pub total_energy_spent: f64,
    /// Current mass stored.
    pub mass_storage: f64,
    /// Mass storage capacity.
    #[serde(default)]
    pub mass_storage_cap: f64,
    /// Current energy stored.
    pub energy_storage: f64,
    /// Energy storage capacity.
    #[serde(default)]
    pub energy_storage_cap: f64,
}

/// Observable event emitted by the simulation each step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimulationEvent {
    /// A tick happened and the economy is in the given state.
    Ticked(EcoSnapshot),
    /// A task has become active.
    TaskStarted { task_id: u32, time: f64 },
    /// A task has finished.
    TaskCompleted { task_id: u32, time: f64 },
    /// The whole queue is done.
    Finished,
}
