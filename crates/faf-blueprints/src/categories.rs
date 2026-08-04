use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Technology tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TechLevel {
    T1,
    T2,
    T3,
    T4,
}

impl TechLevel {
    pub fn new(level_str: &str) -> Result<TechLevel> {
        match level_str {
            "TECH1" => Ok(TechLevel::T1),
            "TECH2" => Ok(TechLevel::T2),
            "TECH3" => Ok(TechLevel::T3),
            "TECH4" => Ok(TechLevel::T4),
            "EXPERIMENTAL" => Ok(TechLevel::T4),
            others => Err(Error::Others(format!("unsupported tech level: {others}"))),
        }
    }
}
