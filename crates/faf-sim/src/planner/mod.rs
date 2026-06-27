//! Planners for FAF build-order generation.
//!
//! This module provides:
//!
//! - The [`Planner`] type and [`Strategy`] registry for searching the
//!   graph-growth model in [`crate::sim`].
//! - A STRIPS/goal-oriented planning layer ([`strips`], [`dependency_graph`])
//!   for reasoning about build/upgrade dependencies symbolically.

pub mod beam;
pub mod core;
pub mod dependency_graph;
pub mod greedy;
pub(crate) mod heuristic;
pub mod mcts;
pub(crate) mod search;
pub mod strips;

pub use core::{PlanResult, Planner, PlannerConfig, PlannerError, Strategy};
pub use dependency_graph::{
    build_dependency_graph, DependencyGraph, DependencyGraphError, DependencyNode,
};
pub use strips::{build_operators, Fact, Operator, StripsAction};
