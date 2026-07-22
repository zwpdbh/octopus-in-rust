//! High-level scheduler application wrapper.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind};
use faf_sim_shared::EcoSnapshot;

use crate::components::{BuildPowerComp, BuilderState, UnitKindComp};
use crate::config::SchedulerConfig;
use crate::plugins::apply::StepReasoningLog;
use crate::plugins::eco::decide_direction::{DirectionScores, PriorityTable};
use crate::plugins::eco::observe::Observation;
use crate::plugins::lifecycle::{
    run_to_completion, run_to_completion_with_reasoning, SchedulerLifecyclePlugin, SchedulerResult,
};
use crate::request::{EcoTarget, SearchOptions};
use crate::resources::{
    CurrentTechLevel, EconomyState, SchedulerClock, SearchGoal, SearchProgress, StepLog, TaskLog,
};
use crate::result::{Schedule, ScheduleError, ScheduleWithReasoning};
use crate::search::{compute_current_tech_level, BlueprintLibraryRef, SearchTarget};

/// A Bevy `App` configured for scheduling.
///
/// Build one with [`SchedulerApp::new_eco`] or [`SchedulerApp::new_unit`], add
/// a mode plugin such as [`EcoSchedulingPlugin`](crate::plugins::eco::EcoSchedulingPlugin)
/// or [`UnitSchedulingPlugin`](crate::plugins::unit::UnitSchedulingPlugin),
/// then call [`SchedulerApp::run_eco`] or [`SchedulerApp::run_unit`] to execute
/// the search.
pub struct SchedulerApp {
    app: App,
}

impl SchedulerApp {
    /// Create a scheduler app for eco scheduling.
    pub fn new_for_eco(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: EcoTarget,
        options: SearchOptions,
        config: SchedulerConfig,
    ) -> Self {
        let mut app = App::new();
        Self::insert_shared_resources(
            &mut app,
            library,
            initial_eco,
            inventory,
            SearchTarget::Eco(target),
            options,
            config,
        );
        app.add_plugins(SchedulerLifecyclePlugin);
        Self { app }
    }

    /// Create a scheduler app for unit scheduling.
    pub fn new_for_unit(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: UnitKind,
        options: SearchOptions,
        config: SchedulerConfig,
    ) -> Self {
        let mut app = App::new();
        Self::insert_shared_resources(
            &mut app,
            library,
            initial_eco,
            inventory,
            SearchTarget::Unit(target),
            options,
            config,
        );
        app.add_plugins(SchedulerLifecyclePlugin);
        Self { app }
    }

    fn insert_shared_resources(
        app: &mut App,
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        inventory: HashMap<UnitKind, u32>,
        target: SearchTarget,
        options: SearchOptions,
        config: SchedulerConfig,
    ) {
        let tech_level = compute_current_tech_level(inventory.keys().cloned());

        let mut commands = app.world_mut().commands();
        for (kind, count) in inventory {
            for _ in 0..count {
                let build_power = library.build_power(&kind);
                commands.spawn((
                    UnitKindComp(kind.clone()),
                    BuildPowerComp(build_power),
                    BuilderState::Idle,
                ));
            }
        }

        app.insert_resource(EconomyState {
            initial: initial_eco,
            current: initial_eco,
        })
        .insert_resource(SchedulerClock {
            now: initial_eco.time,
        })
        .insert_resource(CurrentTechLevel(tech_level))
        .insert_resource(SearchGoal(target))
        .insert_resource(options)
        .insert_resource(SearchProgress {
            iteration: 0,
            next_id: 1,
            done: false,
        })
        .init_resource::<TaskLog>()
        .init_resource::<StepLog>()
        .insert_resource(BlueprintLibraryRef(library))
        .insert_resource(config)
        .init_resource::<Observation>()
        .init_resource::<DirectionScores>()
        .init_resource::<PriorityTable>()
        .init_resource::<SchedulerResult>()
        .init_resource::<StepReasoningLog>();
    }

    /// Add an arbitrary plugin (e.g. a scheduling mode or an algorithm).
    pub fn with_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.app.add_plugins(plugin);
        self
    }

    /// Apply arbitrary configuration to the underlying Bevy `App`.
    ///
    /// This is useful for algorithm implementations that need to register
    /// systems but are only known at runtime through a trait object.
    pub fn configure(mut self, f: impl FnOnce(&mut App)) -> Self {
        f(&mut self.app);
        self
    }

    /// Run the app until an eco scheduling result is produced.
    ///
    /// The app must have a mode plugin registered that generates candidates
    /// and an algorithm plugin that selects them.
    pub fn run_eco(&mut self) -> Result<Schedule, ScheduleError> {
        run_to_completion(&mut self.app)
    }

    /// Run the app until an eco scheduling result is produced, including
    /// per-step candidate reasoning.
    pub fn run_eco_with_reasoning(&mut self) -> Result<ScheduleWithReasoning, ScheduleError> {
        run_to_completion_with_reasoning(&mut self.app)
    }

    /// Run the app until a unit scheduling result is produced.
    ///
    /// The app must have a mode plugin registered that generates candidates
    /// and an algorithm plugin that selects them.
    pub fn run_unit(&mut self) -> Result<Schedule, ScheduleError> {
        run_to_completion(&mut self.app)
    }

    /// Run the app until a unit scheduling result is produced, including
    /// per-step candidate reasoning.
    pub fn run_unit_with_reasoning(&mut self) -> Result<ScheduleWithReasoning, ScheduleError> {
        run_to_completion_with_reasoning(&mut self.app)
    }
}
