//! ECS components used by the scheduler world.

use bevy_ecs::prelude::Component;
use faf_blueprints::UnitKind;
use faf_quantities::Time;
use faf_sim_shared::{Action, BuildTask};

/// Identity of a unit owned by the player in the scheduler world.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct UnitKindComp(pub UnitKind);

/// Build power contributed by this unit when assigned to construction.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BuildPowerComp(pub f64);

/// Whether a builder unit is currently idle or busy working on a task.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum BuilderState {
    /// Available to be assigned to a new task.
    Idle,
    /// Assigned to a task until the given simulation time.
    Busy { task_id: u32, until: Time },
}

/// A task committed by the scheduler, waiting for its assigned builders to finish.
#[derive(Component, Clone, Debug)]
pub struct ScheduledTask {
    pub id: u32,
    pub action: Action,
    /// Specific builder entities assigned to this task.
    pub assigned_builders: Vec<bevy_ecs::prelude::Entity>,
    /// Simulator-facing task description.
    pub build_task: BuildTask,
    pub started_at: Time,
    pub expected_finish: Time,
}

/// Specific builder entities, their kinds, and economic stats chosen for a
/// candidate action.
///
/// This is stored alongside [`crate::search::CandidateAction`] on the same
/// candidate entity so the apply step knows exactly which units to mark busy
/// and the scoring step can simulate without re-querying the world.
#[derive(Component, Clone, Debug)]
pub struct CandidateAssignment(
    pub  Vec<(
        bevy_ecs::prelude::Entity,
        faf_blueprints::UnitKind,
        faf_blueprints::UnitEcoStats,
    )>,
);
