//! Shared action type for the planner and simulator.
//!
//! [`SimAction`] is the command vocabulary shared between planning and
//! simulation. It is emitted by the planner and consumed by the simulator.

use crate::engine::unit_graph::NodeId;
use crate::planner::core::Goal;
use crate::units::UnitKind;

/// Action that produced a successor state during search.
///
/// This is the planner-side analogue of a player command. Keeping it alongside
/// the successor lets a reactive planner emit the concrete command that led to
/// the best ranked state.
#[derive(Debug, Clone, PartialEq)]
pub enum SimAction {
    /// Build a unit with the given builders.
    Build {
        unit_id: UnitKind,
        builders: Vec<NodeId>,
    },
    /// Upgrade an existing unit in-place to a higher-tier blueprint.
    Upgrade {
        target_unit_id: UnitKind,
        old_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Build the abstract goal with the given builders.
    BuildGoal { goal: Goal, builders: Vec<NodeId> },
    /// Assist an active project with additional builders.
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Advance time without issuing a command.
    Wait,
}
