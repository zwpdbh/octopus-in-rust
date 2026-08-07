use faf_blueprints::PlayerEcoMetrics;
use uuid::Uuid;

pub enum SimCmd {
    Start,
    Pause,
    Resume,
    GameSpeed(f64),
}

/// Events emitted by a running simulation back to the service / CLI.
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
