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

pub use faf_blueprints::{AdjacencyBonus, UnitEcoStats};
pub use types::{BuildQueue, BuildTask, EcoSnapshot, SimulationEvent};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use systems::{
    completion_system, eco_system, progress_system, recompute_base_economy_system,
    seed_initial_economy_system, spawn_tasks_system, termination_system,
};

/// Bevy plugin that registers the economy simulation systems.
///
/// Add this to an `App` to drive a build-queue simulation. Per-simulation data
/// (the queue, clock, etc.) is inserted by [`crate::sim::Simulation`].
pub struct BuildQueueSimulationPlugin;

impl Plugin for BuildQueueSimulationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, seed_initial_economy_system)
            .add_systems(
                Update,
                (
                    // Activate pending tasks whose `ready_at` time has arrived and
                    // spawn builder producers plus an ActiveBuildTask for each.
                    spawn_tasks_system,
                    // Aggregate base income, maintenance, and storage from all
                    // Producer and StorageContributor entities into EcoState.
                    recompute_base_economy_system,
                    // Run the global economy tick: apply drains, update storage,
                    // compute the stall factor, and emit an EcoSnapshot event.
                    eco_system,
                    // Advance each ActiveBuildTask by the global effective factor.
                    progress_system,
                    // Finish targets whose remaining work reached zero, spawn their
                    // Producer/StorageContributor/AdjacencyBonusComp, and unlock
                    // the next pending task.
                    completion_system,
                    // Detect whether the queue is empty or max_time was reached and
                    // emit the Finished event.
                    termination_system,
                )
                    .chain(),
            );
    }
}
