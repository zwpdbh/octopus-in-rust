//! Planners for FAF build-order generation.
//!
//! This module provides:
//!
//! - The [`Planner`] type and [`Strategy`] registry for searching the
//!   graph-growth model in [`crate::sim`].
//! - A STRIPS/goal-oriented planning layer ([`strips`], [`plan_graph`]) for
//!   reasoning about build/upgrade dependencies symbolically.

pub mod core;
pub(crate) mod heuristic;
pub mod mcts;
pub mod plan_graph;
pub(crate) mod search;
pub mod strips;

pub use core::{PlanResult, Planner, PlannerConfig, PlannerError, Strategy};
pub use plan_graph::{build_plan_graph, PlanEdgeKind, PlanGraphError};
pub use strips::{build_operators, Fact, Operator, StripsAction};
