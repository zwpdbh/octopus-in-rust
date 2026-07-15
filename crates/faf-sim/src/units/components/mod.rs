//! Bevy ECS components for blueprint entities.
//!
//! Blueprint components are split into two groups:
//!
//! - [`attributes`] — symbolic identity and classification.
//! - [`relationships`] — build/upgrade rules that form the tech-tree graph.
//!
//! Numeric economic attributes (cost, build power, production, storage) are not
//! stored as ECS components. They live in the runtime boundary table owned by
//! [`BlueprintLibrary`](super::BlueprintLibrary) so that the blueprint world
//! remains focused on rules while the simulation runtime owns numbers.

pub mod attributes;
pub mod relationships;
