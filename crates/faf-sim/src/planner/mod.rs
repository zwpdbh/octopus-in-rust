//! Planners for FAF build-order generation.
//!
//! This module provides the [`Planner`] type, the [`Strategy`] registry, and
//! strategy-specific planning functions that search the graph-growth model
//! implemented in [`crate::sim`].

pub mod beam;
pub mod core;
pub mod greedy;
pub(crate) mod heuristic;
pub mod mcts;
pub(crate) mod search;

pub use core::{PlanResult, Planner, PlannerConfig, PlannerError, Strategy};
