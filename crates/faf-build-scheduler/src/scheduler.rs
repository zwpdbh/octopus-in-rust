//! High-level scheduler facade.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use bevy_app::App;
use bevy_app::Plugin;
use bevy_ecs::prelude::*;
use faf_blueprints::{BlueprintLibrary, UnitKind};

use crate::algorithms::{algorithm_by_kind, AlgorithmKind, SchedulingAlgorithm};
use crate::app::SchedulerApp;
use crate::plugins::eco::observe::Observation;
use crate::plugins::{EcoSchedulingPlugin, SchedulerStepEvent, UnitSchedulingPlugin};
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Schedule, ScheduleError, ScheduleWithReasoning, StepReasoning, StepResult};

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

    /// Plan the fastest way to reach the eco target, returning the partial plan
    /// even if the goal is unreachable.
    pub fn schedule_eco_best_effort(&self, request: &EcoScheduleRequest) -> ScheduleWithReasoning {
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
        app.run_eco_best_effort()
    }

    /// Plan the fastest way to build the target unit, returning the partial plan
    /// even if the goal is unreachable.
    pub fn schedule_unit_best_effort(
        &self,
        request: &UnitScheduleRequest,
    ) -> ScheduleWithReasoning {
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
        app.run_unit_best_effort()
    }

    /// Run an eco schedule with an arbitrary user-provided plugin installed.
    ///
    /// This is the integration point for external observers: the CLI uses it to
    /// attach a trace-printing observer, and the web service can use it to
    /// stream or record [`SchedulerStepEvent`]s.
    pub fn schedule_eco_with_plugin<P: Plugin>(
        &self,
        request: &EcoScheduleRequest,
        plugin: P,
    ) -> ScheduleWithReasoning {
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
            .with_plugin(plugin)
            .configure(|app| self.algorithm.configure_app(app));
        app.run_eco_best_effort()
    }

    /// Run an eco schedule and stream each committed step through `event_tx`.
    ///
    /// The caller can set `cancelled` to `true` to stop the search early. The
    /// final result is returned once the search finishes or is cancelled.
    pub fn schedule_eco_stream(
        &self,
        request: &EcoScheduleRequest,
        event_tx: mpsc::Sender<ScheduleStreamEvent>,
        cancelled: Arc<AtomicBool>,
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
            .with_plugin(StreamingPlugin(event_tx))
            .configure(|app| self.algorithm.configure_app(app));
        app.run_eco_with_reasoning_cancellable(cancelled)
    }

    /// Run a unit schedule and stream each committed step through `event_tx`.
    pub fn schedule_unit_stream(
        &self,
        request: &UnitScheduleRequest,
        event_tx: mpsc::Sender<ScheduleStreamEvent>,
        cancelled: Arc<AtomicBool>,
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
            .with_plugin(StreamingPlugin(event_tx))
            .configure(|app| self.algorithm.configure_app(app));
        app.run_unit_with_reasoning_cancellable(cancelled)
    }
}

/// Event emitted for every committed scheduling step when running in streaming
/// mode.
#[derive(Debug, Clone)]
pub struct ScheduleStreamEvent {
    pub step: StepResult,
    pub observation: Observation,
    pub reasoning: StepReasoning,
    pub goal_reached: bool,
}

#[derive(Resource, Clone)]
struct StreamSender(mpsc::Sender<ScheduleStreamEvent>);

fn stream_step_event(trigger: On<SchedulerStepEvent>, sender: Res<StreamSender>) {
    let event = trigger.event();
    let _ = sender.0.send(ScheduleStreamEvent {
        step: event.step.clone(),
        observation: event.observation.clone(),
        reasoning: event.reasoning.clone(),
        goal_reached: event.goal_reached,
    });
}

struct StreamingPlugin(mpsc::Sender<ScheduleStreamEvent>);

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StreamSender(self.0.clone()));
        app.add_observer(stream_step_event);
    }
}

fn count_inventory(items: &[UnitKind]) -> HashMap<UnitKind, u32> {
    let mut counts = HashMap::new();
    for kind in items {
        *counts.entry(kind.clone()).or_insert(0u32) += 1;
    }
    counts
}
