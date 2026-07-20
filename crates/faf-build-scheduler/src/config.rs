//! Global scheduler configuration.

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Global constraints and knobs that apply to a scheduling run.
///
/// This is intentionally separate from the search request (target, economy,
/// inventory) so that callers can impose policy limits without changing the
/// goal.
#[derive(Debug, Clone, Copy, PartialEq, Resource, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum number of mass extractors (including capped variants) that may
    /// exist in the final plan.
    #[serde(default = "default_max_mex_count")]
    pub max_mex_count: u32,
}

impl SchedulerConfig {
    pub fn new(max_mex_count: u32) -> Self {
        Self { max_mex_count }
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_mex_count: default_max_mex_count(),
        }
    }
}

fn default_max_mex_count() -> u32 {
    10
}
