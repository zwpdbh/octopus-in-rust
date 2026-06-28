//! Build simulator for FAF build-order planning.
//!
//! This module combines three responsibilities:
//!
//! 1. **Economy derivation** — computing an [`EconomyState`] from a snapshot of
//!    owned units (production, storage, maintenance) in [`state::derive_economy`].
//! 2. **Graph-growth simulation** — the model where nodes are built units, edges
//!    record builder assignments, and builders are indivisible (one target at a
//!    time). See [`state::GraphState`].
//! 3. **Reactive simulation driver** — [`runner::run_build_order_simulation`] wires
//!    the simulator and planner actors together and drives time forward.

pub mod runner;
pub mod state;

pub use runner::{run_build_order_simulation, SimulationConfig, SimulationError, SimulationResult};
pub use state::{
    derive_economy, BuildEdge, BuildEvent, BuildGraph, GraphSimError, GraphState, NodeId,
    UnitNode, UnitNodeState,
};
