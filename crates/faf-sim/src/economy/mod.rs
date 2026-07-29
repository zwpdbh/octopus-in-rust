//! Pure numerical economy rules for FAF build-order simulation.
//!
//! This module contains no Bevy ECS code. It provides:
//!
//! - [`rules`]: drain rates, stall factors, single-project ticks, and helpers.
//! - [`tick`]: the global/graph tick that combines all active construction
//!   drains and applies FAF-standard mass-income scaling during energy stalls.

pub mod rules;
pub mod tick;

pub use rules::{
    apply_tick, compute_drain, total_build_power, BuildDrain, BuildProject, EcoConsumer, EcoFlow,
    EcoProducer, EffectiveBuildPower, GameEcoMetrics, RequestedBuildPower, ResourceProducer,
    TickOutcome, TickResult,
};
pub use tick::{apply_tick_graph, GraphTickResult};
