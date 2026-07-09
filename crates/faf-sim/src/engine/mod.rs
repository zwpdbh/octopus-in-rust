//! Deterministic tick-based game engine for economy and construction.
//!
//! This module provides the core simulation loop that real RTS engines use:
//! discrete ticks, scheduled commands, and a synchronous state transition. It
//! intentionally avoids async timers and wall-clock time so that episodes are
//! reproducible and easy to test.
//!
//! The engine is split into two pieces:
//!
//! - [`EcoEngine`](crate::engine::EcoEngine) owns the economy state and
//!   the simulation clock. It is unit-agnostic.
//! - [`UnitGraph`](crate::engine::unit_graph::UnitGraph) owns the build graph,
//!   adjacency tracker, build events, and unit knowledge. It derives an economy
//!   state from active units and ticks an externally provided economy.
//!
//! The higher-level [`Simulation`](crate::engine::simulation::Simulation) type
//! coordinates the two and is the public state object used by planners.

pub mod adjacency;
pub mod eco;
pub mod runner;
pub mod simulation;

pub mod tick;
pub mod unit_command;
pub mod unit_graph;

pub use adjacency::{production_multiplier, AdjacencyKind, AdjacencyTracker};
pub use eco::{EcoEngine, EcoEngineError, EcoForecast};
pub use runner::{run_build_order_simulation, SimulationConfig, SimulationError, SimulationResult};
pub use simulation::Simulation;
pub use tick::GameTick;
pub use unit_command::{UnitAction, UnitCommand};
pub use unit_graph::{
    builder_power, derive_economy, BuildEdge, BuildEvent, BuildGraph, GoalProject, GraphSimError,
    NodeId, UnitGraph, UnitNode, UnitNodeState,
};
