//! Deterministic tick-based game engine for economy and construction.
//!
//! This module provides the core simulation loop that real RTS engines use:
//! discrete ticks, scheduled commands, and a synchronous state transition. It
//! intentionally avoids async timers and wall-clock time so that episodes are
//! reproducible and easy to test.
//!
//! The engine is split into two pieces:
//!
//! - [`EcoEngine`](crate::engine::engine::EcoEngine) owns the economy state and
//!   the simulation clock. It is unit-agnostic.
//! - [`UnitGraph`](crate::engine::unit_graph::UnitGraph) owns the build graph,
//!   adjacency tracker, build events, and unit knowledge. It derives an economy
//!   state from active units and ticks an externally provided economy.
//!
//! A higher-level simulation layer will coordinate the two; the legacy
//! [`SimulationState`](crate::engine::simulation_state::SimulationState) remains
//! available while planners and trainers are migrated.

pub mod adjacency;
pub mod engine;
pub mod runner;
pub mod simulation_state;
pub mod tick;
pub mod unit_command;
pub mod unit_graph;

pub use adjacency::{production_multiplier, AdjacencyKind, AdjacencyTracker};
pub use engine::{EcoEngine, EcoForecast};
pub use runner::{run_build_order_simulation, SimulationConfig, SimulationError, SimulationResult};
pub use simulation_state::{GoalProject, SimulationState};
pub use tick::GameTick;
pub use unit_command::{UnitAction, UnitCommand};
pub use unit_graph::{
    builder_power, derive_economy, BuildEdge, BuildEvent, BuildGraph, GraphSimError, NodeId,
    UnitGraph, UnitNode, UnitNodeState,
};
