//! Shared types and computation helpers for Forged Alliance Forever (FAF) units.
//!
//! These types model the slim unit index produced by the ETFreeman unit
//! database generator. They are intended to be usable from the downloader
//! CLI, the WASM plugin, and any future simulation crates.

pub mod index;
pub mod unit;
pub mod unit_extra;
pub mod util;
pub mod weapon;

pub use index::FafUnitIndex;
pub use unit::{SplitDamage, Unit};
pub use unit_extra::*;
pub use weapon::{DepthCharge, FireTargetLayerCapsTable, Projectile, RackBones, Weapon};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sample_index() {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        let index: FafUnitIndex = serde_json::from_str(json).expect("embedded index should parse");
        assert!(!index.units.is_empty(), "index should contain units");
        assert!(index.find_unit("UEL0001").is_some(), "UEF ACU should exist");
    }
}
