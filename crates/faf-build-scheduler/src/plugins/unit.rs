//! Unit scheduling mode plugin.

use std::collections::{HashSet, VecDeque};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use faf_blueprints::{BlueprintGraph, UnitKind};

use crate::config::SchedulerConfig;
use crate::plugins::init::SchedulerSet;
use crate::result::Action;
use crate::search::{
    simulate_with_action, BlueprintLibraryRef, CandidateAction, CandidateScore, SearchState,
    SearchTarget,
};
use crate::util::{count_mex, is_mex};

/// Plugin that registers candidate generation and evaluation for unit
/// scheduling.
pub struct UnitSchedulingPlugin;

impl Plugin for UnitSchedulingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            generate_unit_candidates_system.in_set(SchedulerSet::Generate),
        )
        .add_systems(
            Update,
            evaluate_unit_candidates_system.in_set(SchedulerSet::Evaluate),
        );
    }
}

/// Spawn candidate actions that move toward building the target unit.
///
/// For now this generates every legal build and upgrade action from the current
/// inventory; the evaluation step scores them by symbolic distance to the goal.
pub(crate) fn generate_unit_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    config: Res<SchedulerConfig>,
) {
    if state.done {
        return;
    }

    if state
        .target
        .is_reached(&state.current_eco, &state.inventory)
    {
        return;
    }

    let library = &*library.0;
    let current_mex_count = count_mex(&state.inventory, library);
    let mex_cap = config.max_mex_count;

    // All legal build actions.
    for (builder, count) in &state.inventory {
        if *count == 0 {
            continue;
        }
        for target in library.buildable_by(builder) {
            // Enforce the global mex cap on *new* mass extractors.
            if is_mex(library, &target) && current_mex_count >= mex_cap {
                continue;
            }
            commands.spawn(CandidateAction(Action::Build {
                builder: builder.clone(),
                target,
            }));
        }
    }

    // All legal upgrade actions.
    for (from, count) in &state.inventory {
        if *count == 0 {
            continue;
        }
        for path in library.upgrade_paths(from) {
            for builder in &path.builders {
                if state.inventory.get(builder).copied().unwrap_or(0) > 0 {
                    commands.spawn(CandidateAction(Action::Upgrade {
                        from: from.clone(),
                        to: path.target.clone(),
                        builder: builder.clone(),
                    }));
                }
            }
        }
    }
}

/// Score unit-scheduling candidates by symbolic distance to the target unit.
///
/// Candidates that directly build the target use the simulated completion time;
/// all others are ranked by how many build/upgrade edges separate their result
/// from the goal.
pub(crate) fn evaluate_unit_candidates_system(
    mut commands: Commands,
    state: Res<SearchState>,
    library: Res<BlueprintLibraryRef>,
    candidates: Query<(Entity, &CandidateAction)>,
) {
    if state.done {
        return;
    }

    let SearchTarget::Unit(target) = &state.target else {
        return;
    };

    let library = &*library.0;
    let graph = library.build_graph();
    let max_time = state.options.simulation_max_time_seconds;

    for (entity, action) in candidates.iter() {
        let resulting_unit = resulting_unit(&action.0);
        let score = if resulting_unit == *target {
            // Direct construction of the goal: use the actual simulated time.
            if let Some(result) = simulate_with_action(&state, &action.0, library) {
                let completion = result.tasks.last().cloned().unwrap_or(result.total);
                completion.time_seconds
            } else {
                f64::INFINITY
            }
        } else {
            match distance_to_target(&graph, &resulting_unit, target) {
                Some(distance) => max_time + distance as f64,
                None => f64::INFINITY,
            }
        };

        commands.entity(entity).insert(CandidateScore(score));
    }
}

fn resulting_unit(action: &Action) -> UnitKind {
    match action {
        Action::Build { target, .. } => target.clone(),
        Action::Upgrade { to, .. } => to.clone(),
    }
}

/// Shortest number of build/upgrade steps from `from` to `target` in the
/// symbolic blueprint graph. Returns `None` if the target is unreachable.
fn distance_to_target(graph: &BlueprintGraph, from: &UnitKind, target: &UnitKind) -> Option<usize> {
    if from == target {
        return Some(0);
    }

    let start = graph.node_index(from)?;
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start);
    queue.push_back((start, 0usize));

    while let Some((idx, dist)) = queue.pop_front() {
        let kind = &graph.graph[idx].kind;
        if kind == target {
            return Some(dist);
        }

        for neighbor in graph
            .builds_by(kind)
            .map(|(n, _)| n)
            .chain(graph.upgrades_from(kind).map(|(n, _)| n))
        {
            if visited.insert(neighbor) {
                queue.push_back((neighbor, dist + 1));
            }
        }
    }

    None
}
