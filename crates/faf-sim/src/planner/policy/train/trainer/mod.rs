//! Trainer for the hierarchical policy networks.

mod core;
#[path = "loop.rs"]
mod r#loop;
mod run_episode;
mod update;

pub use core::{AdamOptimizer, Trainer};
