//! Shared search infrastructure for ECS scheduling algorithms.
#![allow(unused)]
use std::collections::HashMap;

use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintLibrary, TechLevel, UnitEcoStats, UnitKind};
use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
use faf_sim_shared::plan_types::{ConstructionItem, ConstructionPlan, UnitSummary};
use faf_sim_shared::{BuildTask, EcoSnapshot, GameEcoMetrics};
use faf_solver::{plan_completion_with_tasks, PlanResult};

use crate::components::{BuildPowerComp, BuilderState, CandidateAssignment, UnitKindComp};
use crate::request::{EcoTarget, SearchOptions};
use crate::resources::{StepLog, TaskLog};
use crate::result::{Action, Schedule, ScheduleError};
use faf_sim_shared::CandidateScoreBreakdown;

/// Resource wrapper so the `BlueprintLibrary` can be inserted into a Bevy
/// `World`.
#[derive(Resource)]
pub(crate) struct BlueprintLibraryRef(pub std::sync::Arc<BlueprintLibrary>);

/// A candidate build/upgrade action considered by a search algorithm.
#[derive(Component)]
pub(crate) struct CandidateAction(pub Action);

/// Score attached to a candidate after evaluation. Higher is better.
#[derive(Component, Clone, Debug)]
pub(crate) struct CandidateScore {
    pub total: f64,
    pub breakdown: Option<CandidateScoreBreakdown>,
}

/// The search goal.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchTarget {
    Eco(EcoTarget),
    Unit(UnitKind),
}

impl SearchTarget {
    /// True if the goal has been met given the current economy and inventory.
    pub fn is_reached(
        &self,
        current_eco: &GameEcoMetrics,
        inventory: &HashMap<UnitKind, u32>,
    ) -> bool {
        match self {
            SearchTarget::Eco(target) => target.is_reached(current_eco),
            SearchTarget::Unit(kind) => inventory.get(kind).copied().unwrap_or(0) > 0,
        }
    }

    /// True if the goal has been met given the current economy and scheduler
    /// unit entities.
    pub fn is_reached_from_entities(
        &self,
        current_eco: &GameEcoMetrics,
        units: &Query<&UnitKindComp>,
    ) -> bool {
        match self {
            SearchTarget::Eco(target) => target.is_reached(current_eco),
            SearchTarget::Unit(kind) => units.iter().any(|u| u.0 == *kind),
        }
    }
}

/// Convert the chosen actions into a final `Schedule`.
pub(crate) fn build_schedule(
    economy: &GameEcoMetrics,
    task_log: &TaskLog,
    step_log: &StepLog,
) -> Result<Schedule, ScheduleError> {
    // let plan = build_construction_plan(economy, task_log);
    // Ok(Schedule {
    //     plan,
    //     steps: step_log.0.clone(),
    //     final_eco: economy.current,
    //     total_time_seconds: step_log
    //         .0
    //         .last()
    //         .map(|s| s.finish_time_seconds)
    //         .unwrap_or(0.0),
    // })
    todo!("not implemented")
}

// fn build_construction_plan(economy_state: &EconomyState, task_log: &TaskLog) -> ConstructionPlan {
//     let items = task_log
//         .0
//         .iter()
//         .map(|task| ConstructionItem {
//             id: task.id,
//             builders: task
//                 .builders
//                 .iter()
//                 .map(UnitSummary::from_builder_stats)
//                 .collect(),
//             targets: task
//                 .targets
//                 .iter()
//                 .map(UnitSummary::from_target_stats)
//                 .collect(),
//             start_after: task.start_after,
//         })
//         .collect();

//     ConstructionPlan {
//         eco: snapshot_to_initial_settings(&economy_state.initial),
//         items,
//     }
// }

/// Build a `BuildTask` representing `action` given the concrete builders
/// assigned to it.
///
/// `assigned_builders` is the list of unit entities, their kinds, and their
/// builder-focused economic stats selected for this action. For builds this is
/// the builder group; for upgrades it starts with the source unit followed by
/// assistants.
pub(crate) fn build_task_for_action(
    action: &Action,
    assigned_builders: &[(Entity, UnitKind, UnitEcoStats)],
    library: &BlueprintLibrary,
    id: u32,
) -> Option<BuildTask> {
    if assigned_builders.is_empty() {
        return None;
    }
    match action {
        Action::Build { target, builder } => {
            if builder.len() != assigned_builders.len() {
                return None;
            }
            if assigned_builders
                .iter()
                .zip(builder)
                .any(|((_, kind, _), expected)| kind != expected)
            {
                return None;
            }
            if assigned_builders
                .iter()
                .any(|(_, _, stats)| stats.build_power <= 0.0)
            {
                return None;
            }
            Some(BuildTask {
                id,
                start_after: Time::from_raw(0.0),
                builders: assigned_builders
                    .iter()
                    .map(|(_, _, stats)| stats.clone())
                    .collect(),
                targets: vec![library.unit_eco_stats(target)?],
            })
        }
        Action::Upgrade {
            from,
            to,
            assisted_by,
        } => {
            let (_, source_kind, source_stats) = &assigned_builders[0];
            if source_kind != from {
                return None;
            }
            // Verify the upgrade or cap target is reachable from `from`.
            let is_upgrade = library.upgrade_target(from) == Some(to.clone());
            let is_cap = library.cap_target(from) == Some(to.clone());
            if !is_upgrade && !is_cap {
                return None;
            }
            let mut builders = vec![source_stats.clone()];
            builders.extend(
                assigned_builders
                    .iter()
                    .skip(1)
                    .map(|(_, _, stats)| stats.clone()),
            );
            // Verify assisted kinds match the action.
            let expected_assist: Vec<_> = assisted_by.iter().cloned().collect();
            let actual_assist: Vec<_> = assigned_builders
                .iter()
                .skip(1)
                .map(|(_, kind, _)| kind.clone())
                .collect();
            if actual_assist != expected_assist {
                return None;
            }
            let target_stats = library.unit_eco_stats(to)?;
            Some(BuildTask {
                id,
                start_after: Time::from_raw(0.0),
                builders,
                targets: vec![target_stats],
            })
        }
    }
}

/// Simulate `action` as the next step from the current search state and
/// return the per-task result.
pub(crate) fn solve_action(
    current_economy: &GameEcoMetrics,
    next_id: u32,
    options: &SearchOptions,
    action: &Action,
    assigned_builders: &[(Entity, UnitKind, UnitEcoStats)],
    library: &BlueprintLibrary,
) -> Option<PlanResult> {
    let task = build_task_for_action(action, assigned_builders, library, next_id)?;
    Some(simulate_action(
        current_economy,
        action,
        &task,
        library,
        options.simulation_max_time_seconds,
    ))
}

/// Simulate a single committed action, adjusting the starting economy so that
/// upgrades correctly replace the source unit's contribution instead of adding
/// to it.
pub(crate) fn simulate_action(
    current_economy: &GameEcoMetrics,
    action: &Action,
    task: &BuildTask,
    library: &BlueprintLibrary,
    max_time_seconds: f64,
) -> PlanResult {
    todo!("not implemented")
}

/// Query type for idle builder units in the scheduler world.
pub(crate) type IdleBuilderQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static UnitKindComp,
        &'static BuildPowerComp,
        &'static BuilderState,
    ),
>;

/// Spawn build candidates using 1, 2, 4, or all available builders of the same
/// kind. More builders speed up construction but tie up more units; the search
/// score decides the best trade-off.
pub(crate) fn spawn_build_candidates(
    commands: &mut Commands,
    library: &BlueprintLibrary,
    builder: &UnitKind,
    target: UnitKind,
    idle_builders: &IdleBuilderQuery,
    eco_snapshot: &GameEcoMetrics,
) {
    let available: Vec<(Entity, UnitKind, UnitEcoStats)> = idle_builders
        .iter()
        .filter(|(_, kind, _, state)| kind.0 == *builder && matches!(state, BuilderState::Idle))
        .map(|(entity, kind, _, _)| {
            let stats = library
                .to_unit_eco_stats(&kind.0, true)
                .expect("owned unit has stats");
            (entity, kind.0.clone(), stats)
        })
        .collect();
    if available.is_empty() {
        return;
    }

    let target_unit_cost = library.unit_build_cost(&target).unwrap();

    let max_bp = faf_solver::solve_approriate_builder_power(
        eco_snapshot,
        target_unit_cost.mass,
        target_unit_cost.energy,
        target_unit_cost.build_time,
    );
    let builder_bp = library.build_power(builder);
    let max_number_builder = max_bp as usize / builder_bp as usize;
    let number_of_builder = max_number_builder.min(available.len());
    // println!(
    //     "number_of_builder : {}, builder_bp: {}, max_number_builder: {}",
    //     number_of_builder, builder_bp, max_number_builder
    // );

    let assigned: Vec<(Entity, UnitKind, UnitEcoStats)> =
        available.iter().take(number_of_builder).cloned().collect();
    commands.spawn((
        CandidateAction(Action::Build {
            builder: vec![builder.clone(); number_of_builder],
            target: target.clone(),
        }),
        CandidateAssignment(assigned),
    ));
}

/// Spawn upgrade/cap candidates without assistance and with 1, 2, or 4
/// assisting engineers of each available tier. Assisting engineers speed up
/// the upgrade but tie up units that could be doing other work.
pub(crate) fn spawn_upgrade_candidates(
    commands: &mut Commands,
    library: &BlueprintLibrary,
    from: &UnitKind,
    to: UnitKind,
    idle_builders: &IdleBuilderQuery,
) {
    // Pick one idle source unit to represent the upgrade. Multiple source units
    // would produce equivalent candidates, so a single representative is enough.
    let source = idle_builders
        .iter()
        .filter(|(_, kind, _, state)| kind.0 == *from && matches!(state, BuilderState::Idle))
        .map(|(entity, kind, _, _)| {
            let stats = library
                .to_unit_eco_stats(&kind.0, true)
                .expect("owned unit has stats");
            (entity, kind.0.clone(), stats)
        })
        .next();
    let Some(source) = source else {
        return;
    };
    let source_entity = source.0;

    // No assistance.
    commands.spawn((
        CandidateAction(Action::Upgrade {
            from: from.clone(),
            to: to.clone(),
            assisted_by: vec![],
        }),
        CandidateAssignment(vec![source.clone()]),
    ));

    // Try assisting with engineers of each available tier.
    for tier in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
        let helper = UnitKind::Engineer(tier);
        let available: Vec<(Entity, UnitKind, UnitEcoStats)> = idle_builders
            .iter()
            .filter(|(entity, kind, _, state)| {
                kind.0 == helper && matches!(state, BuilderState::Idle) && *entity != source_entity
            })
            .map(|(entity, kind, _, _)| {
                let stats = library
                    .to_unit_eco_stats(&kind.0, true)
                    .expect("owned unit has stats");
                (entity, kind.0.clone(), stats)
            })
            .collect();
        if available.is_empty() {
            continue;
        }
        let mut counts = std::collections::BTreeSet::new();
        counts.insert(1usize);
        counts.insert(2.min(available.len()));
        counts.insert(4.min(available.len()));
        for count in counts {
            let mut assigned = vec![source.clone()];
            assigned.extend(available.iter().take(count).cloned());
            commands.spawn((
                CandidateAction(Action::Upgrade {
                    from: from.clone(),
                    to: to.clone(),
                    assisted_by: vec![helper.clone(); count],
                }),
                CandidateAssignment(assigned),
            ));
        }
    }
}
