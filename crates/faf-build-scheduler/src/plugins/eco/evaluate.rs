//! Candidate evaluation for eco scheduling.

use bevy_ecs::prelude::*;

use crate::algorithms::greedy;
use crate::components::CandidateAssignment;
use crate::decision::CurrentEcoDirection;
use crate::request::SearchOptions;
use crate::resources::{EconomyState, SearchGoal, SearchProgress};
use crate::search::{BlueprintLibraryRef, CandidateAction, CandidateScore, SearchTarget};

/// Evaluate every spawned [`CandidateAction`] for eco scheduling and attach a
/// [`CandidateScore`].
///
/// The actual scoring function lives in the algorithm module so that different
/// algorithms can reuse the same ECS pipeline.
pub(crate) fn evaluate_eco_candidates_system(
    mut commands: Commands,
    progress: Res<SearchProgress>,
    economy: Res<EconomyState>,
    goal: Res<SearchGoal>,
    options: Res<SearchOptions>,
    library: Res<BlueprintLibraryRef>,
    direction: Res<CurrentEcoDirection>,
    candidates: Query<(Entity, &CandidateAction, &CandidateAssignment)>,
) {
    if progress.done {
        return;
    }

    let SearchTarget::Eco(_) = &goal.0 else {
        return;
    };

    let library = &*library.0;

    for (entity, action, assignment) in candidates.iter() {
        let score = greedy::score_eco_candidate(
            &economy.current,
            progress.next_id,
            &options,
            &action.0,
            &assignment.0,
            library,
            direction.0,
        );
        commands.entity(entity).insert(CandidateScore(score));
    }
}
