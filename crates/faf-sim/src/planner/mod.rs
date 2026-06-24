//! Planners for FAF build-order generation.
//!
//! This module provides the [`Planner`] trait, the [`Strategy`] registry, and
//! concrete planner implementations that search the graph-growth model
//! implemented in [`crate::sim`].

pub mod beam;
pub mod core;
pub mod greedy;
mod heuristic;
mod search;

pub use beam::BeamPlanner;
pub use core::{build_planner, PlanResult, Planner, PlannerError, Strategy};
pub use greedy::GreedyPlanner;
