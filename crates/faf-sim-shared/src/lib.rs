//! Shared types and conversions for FAF simulator consumers.
//!
//! This crate sits between `faf-sim` (the Bevy ECS runtime) and `faf-solver`
//! (the analytical solver), providing stable, serializable plan and economy
//! representations that can be used without pulling in Bevy.

pub mod construction_types;

pub use construction_types::{
    BuildQueue, ConstructionTask, PlayerEcoMetrics, PlayerEcoSnapshot, EPS,
};
