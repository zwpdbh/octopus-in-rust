//! Shared types and conversions for FAF simulator consumers.
//!
//! This crate sits between `faf-sim` and the various frontends/tools, providing
//! a stable, serializable plan representation (`ConstructionPlan`) that can be
//! converted to and from the simulator's `BuildQueue`.

pub mod plan;

pub use plan::{ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary};
