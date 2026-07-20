//! High-level scheduler application wrapper.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::prelude::*;

use faf_blueprints::{BlueprintLibrary, UnitKind};
use faf_sim::runtime::EcoSnapshot;

use crate::plugins::scheduler::{run_to_completion, SchedulerInitPlugin};
use crate::request::{EcoTarget, SearchOptions};
use crate::result::{Schedule, ScheduleError};

/// A Bevy `App` configured for scheduling.
///
/// Build one with [`SchedulerApp::new_eco`] or [`SchedulerApp::new_unit`], add
/// mode plugins such as [`EcoSchedulingPlugin`](crate::eco_plugin::EcoSchedulingPlugin)
/// or [`UnitSchedulingPlugin`](crate::unit_plugin::UnitSchedulingPlugin), and an
/// algorithm plugin such as [`GreedyPlugin`](crate::algorithms::GreedyPlugin),
/// then call [`SchedulerApp::run_eco`] or [`SchedulerApp::run_unit`] to execute
/// the search.
pub struct SchedulerApp {
    app: App,
}

impl SchedulerApp {
    /// Create a scheduler app for eco scheduling.
    pub fn new_eco(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        initial_inventory: HashMap<UnitKind, u32>,
        target: EcoTarget,
        options: SearchOptions,
    ) -> Self {
        let mut app = App::new();
        app.add_plugins(SchedulerInitPlugin::new_eco(
            library,
            initial_eco,
            initial_inventory,
            target,
            options,
        ));
        Self { app }
    }

    /// Create a scheduler app for unit scheduling.
    pub fn new_unit(
        library: Arc<BlueprintLibrary>,
        initial_eco: EcoSnapshot,
        initial_inventory: HashMap<UnitKind, u32>,
        target: UnitKind,
        options: SearchOptions,
    ) -> Self {
        let mut app = App::new();
        app.add_plugins(SchedulerInitPlugin::new_unit(
            library,
            initial_eco,
            initial_inventory,
            target,
            options,
        ));
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
