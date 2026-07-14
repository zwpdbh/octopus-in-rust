//! Observable, steppable Bevy ECS economy runtime.
//!
//! The runtime wires the pure numerical rules from [`crate::economy`] into a
//! deterministic Bevy `App`. The public types live in [`types`]; ECS internals
//! are split into [`components`], [`resources`], and [`systems`].

pub(crate) mod components;
pub(crate) mod resources;
mod systems;
pub mod types;

#[cfg(test)]
mod tests;

pub use types::{BuildQueue, BuildTask, EcoSnapshot, SimulationEvent, UnitEcoStats};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use systems::{
    completion_system, eco_system, progress_system, recompute_base_economy_system,
    spawn_tasks_system, termination_system,
};

/// Bevy plugin that registers the economy simulation systems.
///
/// Add this to an `App` to drive a build-queue simulation. Per-simulation data
/// (the queue, clock, etc.) is inserted by [`crate::sim::Simulation`].
pub struct EcoPlugin;

impl Plugin for EcoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_tasks_system,
                recompute_base_economy_system,
                eco_system,
                progress_system,
                completion_system,
                termination_system,
            )
                .chain(),
        );
    }
}
