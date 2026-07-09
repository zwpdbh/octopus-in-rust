//! Commands that can be issued to the unit graph.
//!
//! A command is an action stamped with the simulation tick on which it should
//! take effect. For single-agent eco training commands are usually scheduled for
//! the current tick; a multiplayer-ready engine would schedule them several ticks
//! in the future to account for network latency.
//!
//! The tick stamp is produced by [`EcoEngine`](crate::engine::EcoEngine), but the
//! action itself is interpreted by [`UnitGraph`](crate::engine::UnitGraph).

use crate::engine::unit_graph::NodeId;
use crate::units::UnitKind;

use super::tick::GameTick;

/// A scheduled unit-level action.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitCommand {
    /// The tick on which the action takes effect.
    pub tick: GameTick,
    /// The action to perform.
    pub action: UnitAction,
}

impl UnitCommand {
    /// Create a command that takes effect on the given tick.
    pub fn new(tick: GameTick, action: UnitAction) -> Self {
        Self { tick, action }
    }

    /// Create a command that takes effect immediately on the current tick.
    pub fn now(action: UnitAction) -> Self {
        Self {
            tick: GameTick::FIRST,
            action,
        }
    }
}

/// An action that changes the build state of the unit graph.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitAction {
    /// Start building a unit with the given idle builders.
    Build {
        /// Blueprint of the unit to construct.
        unit: UnitKind,
        /// Node ids of the builders that will work on the project.
        builders: Vec<NodeId>,
    },
    /// Assign additional idle builders to an active project.
    Assist {
        /// Node id of the project being assisted.
        project: NodeId,
        /// Node ids of the builders to add.
        builders: Vec<NodeId>,
    },
    /// Upgrade an existing unit in-place to a higher-tier blueprint.
    Upgrade {
        /// Blueprint of the unit to upgrade into.
        target: UnitKind,
        /// Node id of the unit being upgraded.
        old_node: NodeId,
        /// Node ids of the builders that will work on the upgrade.
        builders: Vec<NodeId>,
    },
}
