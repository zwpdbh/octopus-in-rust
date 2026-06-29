//! Messages exchanged between the simulation actor and decision actor.
//!
//! The architecture treats `SimActor` and `DecisionActor` as two concurrent
//! components that communicate only through these messages. This mirrors how a
//! human player observes the game and sends discrete commands.

use crate::sim::{BuildEvent, GraphState, NodeId};
use crate::units::UnitKind;

/// Command sent from the planner to the simulation.
///
/// The planner never advances time directly; it only tells the simulation what
/// to build or assist. The simulation advances fixed ticks and reports back.
#[derive(Debug, Clone)]
pub enum Command {
    /// Start building a unit with the given idle builders.
    Build {
        /// Blueprint id of the unit to build.
        unit_id: UnitKind,
        /// Builder nodes that will work on the project.
        builders: Vec<NodeId>,
    },
    /// Assign additional idle builders to an active project.
    Assist {
        /// Node id of the project being assisted.
        project_node: NodeId,
        /// Builder nodes to add to the project.
        builders: Vec<NodeId>,
    },
    /// Upgrade an existing unit in-place to a higher-tier blueprint.
    Upgrade {
        /// Blueprint id of the unit to upgrade into.
        target_unit_id: UnitKind,
        /// Node id of the unit being upgraded.
        old_node: NodeId,
        /// Builder nodes that will work on the upgrade.
        builders: Vec<NodeId>,
    },
}

/// Observation sent from the simulation to the planner.
///
/// The simulation owns the authoritative `GraphState`. After each tick it sends
/// a snapshot to the planner so the planner can decide the next command.
#[derive(Debug, Clone)]
pub enum Observation {
    /// A build event occurred during the last tick.
    Event(BuildEvent),
    /// The current simulation state at the end of a tick.
    State(GraphState),
}
