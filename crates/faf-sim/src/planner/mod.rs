//! Planners for FAF build-order generation.
//!
//! This module provides:
//!
//! - The [`Planner`] type and [`Strategy`] registry for searching the
//!   graph-growth model in [`crate::sim`].
//! - A STRIPS/goal-oriented planning layer (`plan_graph`) for
//!   reasoning about build/upgrade dependencies symbolically.

pub(crate) mod action;
pub mod core;
pub mod plan_graph;
pub mod policy;

pub use action::SimAction;
pub use core::{Goal, PlanResult, Planner, PlannerConfig, PlannerError, Strategy, ValueNetKind};
pub use plan_graph::{build_plan_graph, EdgeAction, PlanGraph, PlanNode};
