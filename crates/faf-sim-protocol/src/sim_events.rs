use faf_blueprints::{ConstructionPlan, PlayerEcoMetrics};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Commands sent from the service / CLI into a running simulation thread.
///
/// These cross the normal-application → Bevy-app boundary via the
/// `crossbeam_channel` held by `SimulationHandle` in `faf-sim-service`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimCmd {
    Start,
    Pause,
    Resume,
    /// Change playback speed. The engine itself is tick-based; speed is
    /// realized by the service thread throttling how often it calls
    /// `app.update()`.
    GameSpeed(SimSpeed),
}

/// Simulation playback speed.
///
/// The engine processes one fixed tick per `app.update()`.  One tick
/// represents one simulation second.  Real-world cadence is controlled
/// by sleeping between ticks in `faf-sim-service::run_sim_thread`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SimSpeed {
    /// Run the simulation as fast as the CPU allows (default for headless runs).
    Unlimited,
    /// Run at a fixed number of simulation ticks per wall-clock second.
    /// In this mode one tick represents one simulation second.
    TicksPerSecond(f64),
}

impl SimSpeed {
    /// Default speed used when none is specified.
    pub fn default() -> Self {
        SimSpeed::Unlimited
    }

    /// Number of wall-clock seconds to wait between ticks, if any.
    pub fn tick_interval_seconds(&self) -> Option<f64> {
        match self {
            SimSpeed::Unlimited => None,
            SimSpeed::TicksPerSecond(rate) => Some(1.0 / rate),
        }
    }
}

/// Events emitted by a running simulation back to the service / CLI.
///
/// These cross the Bevy-app → normal-application boundary via the
/// `EventSender` resource held by the engine world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimEvent {
    EcoSummary(PlayerEcoMetrics),
    ActionFinished(Uuid),
}

impl std::fmt::Display for SimEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimEvent::EcoSummary(eco) => write!(f, "eco summary => {:?}", eco),
            SimEvent::ActionFinished(task_id) => write!(f, "action finished => {task_id}"),
        }
    }
}

/// Messages the frontend sends to the simulation server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimClientMessage {
    StartPlan {
        plan: ConstructionPlan,
        speed: SimSpeed,
    },
    Command(SimCmd),
}

/// Messages the simulation server sends to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimServerMessage {
    Event(SimEvent),
    Error(String),
    Finished,
}
