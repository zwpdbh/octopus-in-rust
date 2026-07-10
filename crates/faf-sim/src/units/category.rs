//! Unit category classification for UI grouping.

use super::kind::UnitKind;

/// Broad category used to group units in build palettes and summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnitCategory {
    Commander,
    Engineer,
    Factory,
    Economic,
    Military,
    Other,
}

impl UnitCategory {
    /// Human-readable label for the category.
    pub fn label(self) -> &'static str {
        match self {
            UnitCategory::Commander => "Commander",
            UnitCategory::Engineer => "Engineers",
            UnitCategory::Factory => "Factories",
            UnitCategory::Economic => "Economic",
            UnitCategory::Military => "Military",
            UnitCategory::Other => "Other",
        }
    }
}

/// Classify a unit kind into a UI category.
///
/// Common economic/builder kinds are derived directly from their variant. Unique
/// units are bucketed as military or other based on their blueprint id prefix.
pub fn category_of(kind: &UnitKind) -> UnitCategory {
    match kind {
        UnitKind::Commander => UnitCategory::Commander,
        UnitKind::Engineer(_) => UnitCategory::Engineer,
        UnitKind::Factory(_) => UnitCategory::Factory,
        UnitKind::Mex(_)
        | UnitKind::Pgen(_)
        | UnitKind::EnergyStorage
        | UnitKind::CapT2Mex
        | UnitKind::CapT3Mex => UnitCategory::Economic,
        UnitKind::Unique(id) => {
            // FAF blueprint ids use the second character to indicate tech/role:
            // A = air, L = land, S = structure, R = robot/bot, B = bomber, etc.
            // The first character is faction; the third is tech tier.
            let s = id.0.as_str();
            if s.len() >= 2 {
                match &s[1..2] {
                    "A" | "L" | "R" | "B" | "S"
                        if s.len() >= 3 && s[2..3].parse::<u8>().is_ok() =>
                    {
                        UnitCategory::Military
                    }
                    _ => UnitCategory::Other,
                }
            } else {
                UnitCategory::Other
            }
        }
    }
}
