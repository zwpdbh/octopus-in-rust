use crate::resources::Time;
use bevy_ecs::prelude::*;

/// Advance the simulation clock by one tick.
///
/// Because one tick represents one simulation second in the current model,
/// this simply adds `time.delta_seconds` (normally `1.0`) to
/// `time.elapsed_seconds`.
pub fn advance_time(mut time: ResMut<Time>) {
    time.elapsed_seconds += time.delta_seconds;
}
