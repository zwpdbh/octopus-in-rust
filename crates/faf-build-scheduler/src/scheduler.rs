//! High-level scheduler facade.

use std::collections::HashMap;
use std::sync::Arc;

use faf_blueprints::{BlueprintLibrary, UnitKind};

use crate::algorithms::{algorithm_by_kind, AlgorithmKind, SchedulingAlgorithm};
use crate::app::SchedulerApp;
use crate::plugins::{EcoSchedulingPlugin, UnitSchedulingPlugin};
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError, ScheduleWithReasoning};

/// Build-order scheduler.
///
/// Holds a `BlueprintLibrary` and dispatches requests to a selectable
/// [`SchedulingAlgorithm`].
pub struct Scheduler {
    library: Arc<BlueprintLibrary>,
    algorithm: Box<dyn SchedulingAlgorithm>,
}

impl Scheduler {
    /// Create a scheduler with the given algorithm.
    pub fn with_algorithm(library: BlueprintLibrary, kind: AlgorithmKind) -> Self {
        Self {
            library: Arc::new(library),
            algorithm: algorithm_by_kind(kind),
        }
    }

    /// Create a scheduler using the default greedy algorithm.
    pub fn new(library: BlueprintLibrary) -> Self {
        Self::with_algorithm(library, AlgorithmKind::Greedy)
    }

    /// Create a scheduler using the default FAF units database shipped with the
    /// workspace.
    pub fn from_default_units(kind: AlgorithmKind) -> anyhow::Result<Self> {
        let library = BlueprintLibrary::from_default_units()?;
        Ok(Self::with_algorithm(library, kind))
    }

    /// Plan the fastest way to reach the eco target.
    pub fn schedule_eco(&self, request: &EcoScheduleRequest) -> Result<Schedule, ScheduleError> {
        let inventory = count_inventory(&request.initial_inventory);
        let mut app = SchedulerApp::new_for_eco(
            Arc::clone(&self.library),
            request.initial_eco,
            inventory,
            request.target.clone(),
            request.options.clone(),
            request.config,
        );
        app = app
            .with_plugin(EcoSchedulingPlugin)
            .configure(|app| self.algorithm.configure_app(app));
        app.run_eco()
    }

    /// Plan the fastest way to reach the eco target, including per-step
    /// candidate reasoning.
    pub fn schedule_eco_with_reasoning(
        &self,
        request: &EcoScheduleRequest,
    ) -> Result<ScheduleWithReasoning, ScheduleError> {
        let inventory = count_inventory(&request.initial_inventory);
        let mut app = SchedulerApp::new_for_eco(
            Arc::clone(&self.library),
            request.initial_eco,
            inventory,
            request.target.clone(),
            request.options.clone(),
            request.config,
        );
        app = app
            .with_plugin(EcoSchedulingPlugin)
            .configure(|app| self.algorithm.configure_app(app));
        app.run_eco_with_reasoning()
    }

    /// Plan the fastest way to build the target unit.
    pub fn schedule_unit(&self, request: &UnitScheduleRequest) -> Result<Schedule, ScheduleError> {
        let inventory = count_inventory(&request.initial_inventory);
        let mut app = SchedulerApp::new_for_unit(
            Arc::clone(&self.library),
            request.initial_eco,
            inventory,
            request.target.clone(),
            request.options.clone(),
            request.config,
        );
        app = app
            .with_plugin(UnitSchedulingPlugin)
            .configure(|app| self.algorithm.configure_app(app));
        app.run_unit()
    }

    /// Plan the fastest way to build the target unit, including per-step
    /// candidate reasoning.
    pub fn schedule_unit_with_reasoning(
        &self,
        request: &UnitScheduleRequest,
    ) -> Result<ScheduleWithReasoning, ScheduleError> {
        let inventory = count_inventory(&request.initial_inventory);
        let mut app = SchedulerApp::new_for_unit(
            Arc::clone(&self.library),
            request.initial_eco,
            inventory,
            request.target.clone(),
            request.options.clone(),
            request.config,
        );
        app = app
            .with_plugin(UnitSchedulingPlugin)
            .configure(|app| self.algorithm.configure_app(app));
        app.run_unit_with_reasoning()
    }
}

fn count_inventory(items: &[UnitKind]) -> HashMap<UnitKind, u32> {
    let mut counts = HashMap::new();
    for kind in items {
        *counts.entry(kind.clone()).or_insert(0u32) += 1;
    }
    counts
}
