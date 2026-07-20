//! Greedy best-first scheduling algorithm.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use faf_sim::units::{BlueprintLibrary, TechLevel, UnitKind};

use crate::algorithms::SchedulingAlgorithm;
use crate::request::{EcoScheduleRequest, UnitScheduleRequest};
use crate::result::{Action, Schedule, ScheduleError};
use crate::search::{
    apply_action_to_inventory, build_schedule, build_task_for_action, score_result,
    simulate_with_action, BlueprintLibraryRef, CandidateAction, CandidateScore, SearchState,
};

/// Greedy best-first scheduler.
///
/// Each search step generates every legal build/upgrade action from the current
/// inventory, simulates the plan with that action appended, and picks the
/// action that reaches the eco target fastest (or makes the most progress per
/// second if the target is not yet reachable).
#[derive(Debug, Default)]
pub struct Greedy;

impl Greedy {
    /// Create a new greedy scheduler.
    pub fn new() -> Self {
        Self
    }
}

impl SchedulingAlgorithm for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn schedule_eco(
        &self,
        library: Arc<BlueprintLibrary>,
        request: &EcoScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        let inventory = count_inventory(&request.initial_inventory);
        let state = SearchState::new(
            request.initial_eco,
            inventory,
            request.target.clone(),
            request.options.clone(),
        );

        let mut app = App::new();
        app.insert_resource(state)
            .insert_resource(BlueprintLibraryRef(library))
            .add_systems(
                Update,
                (
                    generate_candidates_system,
                    evaluate_candidates_system,
                    select_best_system,
                )
                    .chain(),
            );

        while !app.world().resource::<SearchState>().done {
            app.update();
        }

        app.world()
            .resource::<SearchState>()
            .result
            .clone()
            .unwrap_or(Err(ScheduleError::GoalUnreachable))
    }

    fn schedule_unit(
        &self,
        _library: Arc<BlueprintLibrary>,
        _request: &UnitScheduleRequest,
    ) -> Result<Schedule, ScheduleError> {
        todo!("implement greedy unit scheduling")
    }
}

/// Count occurrences of each kind in the initial inventory.
fn count_inventory(items: &[UnitKind]) -> HashMap<UnitKind, u32> {
    let mut counts = HashMap::new();
    for kind in items {
        *counts.entry(kind.clone()).or_insert(0) += 1;
    }
    counts
}

/// Unit kinds the scheduler will consider building directly.
fn eco_build_targets() -> Vec<UnitKind> {
    vec![
        UnitKind::Mex(TechLevel::T1),
        UnitKind::Mex(TechLevel::T2),
        UnitKind::Mex(TechLevel::T3),
        UnitKind::Pgen(TechLevel::T1),
        UnitKind::Pgen(TechLevel::T2),
        UnitKind::Pgen(TechLevel::T3),
        UnitKind::EnergyStorage,
        UnitKind::CapMex(TechLevel::T2),
        UnitKind::CapMex(TechLevel::T3),
    ]
}

/// Spawn `CandidateAction` entities for every legal action from the current
/// inventory.
fn generate_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
) {
    if state.done || state.target.is_reached(&state.current_eco) {
        return;
    }

    let library = &*library.0;
    let mut actions = Vec::new();

    // Build actions: every owned builder kind can build every eco target it is
    // allowed to build.
    for builder in state.inventory.keys() {
        for target in eco_build_targets() {
            if library.can_build(builder, &target) {
                actions.push(Action::Build {
                    target: target.clone(),
                    builder: builder.clone(),
                });
            }
        }
    }

    // Upgrade actions: every owned unit can follow its upgrade paths using any
    // available builder listed in the path.
    for from in state.inventory.keys() {
        for path in library.upgrade_paths(from) {
            for builder in &path.builders {
                if state.inventory.contains_key(builder) {
                    actions.push(Action::Upgrade {
                        from: from.clone(),
                        to: path.target.clone(),
                        builder: builder.clone(),
                    });
                }
            }
        }
    }

    for action in actions {
        commands.spawn(CandidateAction(action));
    }
}

/// Simulate each candidate action and attach a score. Lower score is better.
fn evaluate_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction)>,
) {
    if state.done {
        return;
    }

    let library = &*library.0;

    for (entity, action) in candidates.iter() {
        let score = if let Some(result) = simulate_with_action(&state, &action.0, library) {
            let completion = result.tasks.last().cloned().unwrap_or(result.total);
            score_result(
                &completion,
                &state.target,
                state.options.simulation_max_time_seconds,
            )
        } else {
            f64::INFINITY
        };

        commands.entity(entity).insert(CandidateScore(score));
    }
}

/// Pick the lowest-score candidate, apply it to the search state, and finish
/// if the target is reached or limits are hit.
fn select_best_system(
    mut commands: Commands,
    mut state: ResMut<SearchState>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction, &CandidateScore)>,
) {
    if state.done {
        return;
    }

    let library = &*library.0;

    // Collect to avoid borrowing the query while mutating state.
    let collected: Vec<_> = candidates
        .iter()
        .map(|(e, a, s)| (e, a.0.clone(), s.0))
        .collect();

    let should_finish = if let Some((_, action, score)) = collected
        .iter()
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    {
        if score.is_infinite() {
            state.result = Some(Err(ScheduleError::GoalUnreachable));
            true
        } else {
            let task = build_task_for_action(action, &state.inventory, library, state.next_id)
                .expect("best candidate must be buildable");

            let result = simulate_with_action(&state, action, library)
                .expect("best candidate must simulate");
            let completion = result.tasks.last().cloned().unwrap_or(result.total);
            apply_action_to_inventory(action, &mut state.inventory);
            state.current_eco = completion.economy;
            state.tasks.push(task);
            state.steps.push(crate::result::StepResult {
                action: action.clone(),
                finish_time_seconds: completion.time_seconds,
                economy: completion.economy,
            });
            state.next_id += 1;
            state.iteration += 1;

            if state.target.is_reached(&state.current_eco) {
                state.result = Some(build_schedule(&state));
                true
            } else if state.should_terminate() {
                state.result = Some(Err(ScheduleError::GoalUnreachable));
                true
            } else {
                false
            }
        }
    } else {
        state.result = Some(Err(ScheduleError::GoalUnreachable));
        true
    };

    if should_finish {
        state.done = true;
    }

    // Despawn all candidate entities so the next iteration starts clean.
    for (entity, _, _) in candidates.iter() {
        commands.entity(entity).despawn();
    }
}
