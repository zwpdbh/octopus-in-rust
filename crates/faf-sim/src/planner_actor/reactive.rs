//! Reactive adapter that turns any synchronous [`Planner`](crate::planner::Planner)
//! into an actor-model [`Planner`](super::Planner).
//!
//! On each state observation the adapter runs the underlying synchronous planner
//! from the observed state, extracts the first action of the resulting plan, and
//! emits the corresponding command. This keeps the search/lookahead logic in one
//! place (the synchronous planner) while still allowing the actor loop to drive
//! the simulation step by step.

use faf_units::{DataIndex, Unit};

use crate::message::{Command, Observation};
use crate::planner::core::Planner as SyncPlanner;
use crate::planner::search::SearchAction;
use crate::planner_actor::Planner;

/// A reactive planner that wraps a synchronous planner and emits one command per
/// observation.
///
/// The generic parameter `P` is any type that implements
/// [`crate::planner::Planner`], e.g. [`crate::planner::BeamPlanner`] or
/// [`crate::planner::GreedyPlanner`].
pub struct ReactivePlanner<P: SyncPlanner> {
    /// Underlying synchronous planner.
    planner: P,
    /// Unit database.
    index: DataIndex,
    /// Goal unit to build.
    goal: Unit,
}

impl<P: SyncPlanner> ReactivePlanner<P> {
    /// Create a new reactive adapter around the given synchronous planner.
    pub fn new(planner: P, index: DataIndex, goal: Unit) -> Self {
        Self {
            planner,
            index,
            goal,
        }
    }
}

impl<P: SyncPlanner + Send + Sync> Planner for ReactivePlanner<P> {
    fn decide(&self, observation: &Observation) -> Option<Command> {
        match observation {
            Observation::State(state) => {
                let plan = self
                    .planner
                    .plan(&self.index, state.clone(), &self.goal)
                    .ok()?;
                plan.first_action.and_then(search_action_to_command)
            }
            // Events alone do not trigger a new decision; wait for the next state snapshot.
            Observation::Event(_) => None,
        }
    }
}

fn search_action_to_command(action: SearchAction) -> Option<Command> {
    match action {
        SearchAction::Build { unit_id, builders } => Some(Command::Build { unit_id, builders }),
        SearchAction::Assist {
            project_node,
            builders,
        } => Some(Command::Assist {
            project_node,
            builders,
        }),
        SearchAction::Wait => None,
    }
}
