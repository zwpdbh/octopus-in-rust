//! High-level scheduler application wrapper.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind};
use faf_sim_shared::EcoSnapshot;

use crate::config::SchedulerConfig;
use crate::plugins::lifecycle::{run_to_completion, SchedulerLifecyclePlugin, SchedulerResult};
use crate::request::{EcoTarget, SearchOptions};
use crate::result::{Schedule, ScheduleError};
use crate::search::{BlueprintLibraryRef, SearchState, SearchTarget};

/// A Bevy `App` configured for scheduling.
///
/// Build one with [`SchedulerApp::new_eco`] or [`SchedulerApp::new_unit`], add
/// mode plugins such as [`EcoSchedulingPlugin`](crate::plugins::eco::EcoSchedulingPlugin)
/// or [`UnitSchedulingPlugin`](crate::plugins::unit::UnitSchedulingPlugin), and an
/// algorithm plugin such as [`GreedyPlugin`](crate::plugins::greedy::GreedyPlugin),
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
        app.insert_resource(SearchState::new(
            initial_eco,
            inventory,
            SearchTarget::Eco(target),
            options,
        ))
        .insert_resource(BlueprintLibraryRef(library))
        .insert_resource(config)
        .init_resource::<SchedulerResult>()
        .add_plugins(SchedulerLifecyclePlugin);
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
        app.insert_resource(SearchState::new(
            initial_eco,
            inventory,
            SearchTarget::Unit(target),
            options,
        ))
        .insert_resource(BlueprintLibraryRef(library))
        .insert_resource(config)
        .init_resource::<SchedulerResult>()
        .add_plugins(SchedulerLifecyclePlugin);
        Self { app }
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

    /// Run the app until a unit scheduling result is produced.
    ///
    /// The app must have a mode plugin registered that generates candidates
    /// and an algorithm plugin that selects them.
    pub fn run_unit(&mut self) -> Result<Schedule, ScheduleError> {
        run_to_completion(&mut self.app)
    }
}
