//! ECS-backed blueprint library and unit knowledge for FAF.
//!
//! `faf-blueprints` is the single source of truth for unit kinds, factions,
//! tech levels, build/upgrade rules, and the [`BlueprintLibrary`] that indexes
//! them. It sits on top of the raw `faf-units` parser and is used by the
//! simulator, scheduler, predictor, CLI, backend, and frontend.

mod blueprint;
mod categories;
mod eco_metrics;
mod error;

pub use blueprint::*;
pub use error::*;
