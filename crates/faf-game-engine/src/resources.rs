use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;

/// Live economy snapshot for the simulated player.
///
/// Seeded from `ConstructionPlan.player_eco` and mutated by the eco systems
/// as construction drains / completes.
#[derive(Resource, Default)]
pub struct PlayerEco(pub PlayerEcoMetrics);

impl PlayerEco {}

/// Wall-clock independent simulation clock.
///
/// In the current model one `app.update()` call equals one simulation tick,
/// and one tick represents one simulation second.  `delta_seconds` is therefore
/// `1.0` unless a caller intentionally scales time (not yet supported).
///
/// Real-world playback speed is controlled by the service thread, which
/// decides how often to call `app.update()`.
#[derive(Resource, Default)]
pub struct Time {
    /// Simulation seconds advanced by the current tick.
    pub delta_seconds: f64,
    /// Total simulation seconds elapsed since the simulation started.
    pub elapsed_seconds: f64,
}

impl Time {
    pub fn new(delta_seconds: f64) -> Self {
        Self {
            delta_seconds,
            elapsed_seconds: 0.0,
        }
    }
}
