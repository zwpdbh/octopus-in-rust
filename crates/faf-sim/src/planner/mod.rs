//! Planners for FAF build-order generation.
//!
//! This module provides:
//!
//! - The [`Planner`] type and [`Strategy`] registry for searching the
//!   graph-growth model in [`crate::sim`].
//! - A STRIPS/goal-oriented planning layer (`plan_graph`) for
//!   reasoning about build/upgrade dependencies symbolically.
//! - Independent eco and rush planners (`eco_planner`, `rush_planner`) that
//!   can be composed later into a higher-level strategy.

pub(crate) mod action;
pub mod core;
pub mod eco_planner;
pub mod plan_graph;
pub mod policy;
pub mod rush_planner;

pub use action::SimAction;
pub use core::{Goal, PlanResult, Planner, PlannerConfig, PlannerError, Strategy, ValueNetKind};
pub use eco_planner::{EcoPlanner, DEFAULT_TARGET_MASS_INCOME};
pub use plan_graph::{build_plan_graph, EdgeAction, PlanGraph, PlanNode};
pub use rush_planner::{RushAssessment, RushPlanner};
