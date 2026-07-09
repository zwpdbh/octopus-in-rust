//! Bevy-powered eco/build simulator for Forged Alliance Forever (FAF).
//!
//! This crate now focuses on an interactive build simulator. It keeps the
//! `units` and `economy` math from `faf-units` and adds a minimal Bevy game
//! plugin (`game::EcoSimPlugin`) that can run both natively and on the web.

pub mod economy;
pub mod game;
pub mod quantities;
pub mod units;

pub use economy::{
    apply_tick, apply_tick_graph, compute_drain, total_build_power, BuildDrain, BuildProject,
    EcoFlow, EconomyState, EffectiveBuildPower, GraphTickResult, RequestedBuildPower,
    ResourceProducer, TickOutcome, TickResult,
};
pub use quantities::{BuildPower, BuildWork, Energy, EnergyRate, Mass, MassRate, Time};
pub use units::{
    BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind, UnitRole, Units,
    UpgradeRecipe,
};

/// Run the interactive eco simulator.
///
/// This is the entry point used by both the native CLI and the WASM build.
pub fn run_app() {
    game::run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_units() -> Units {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn monkeylord_requires_t3_engineer() {
        let units = load_units();

        let builders = units.builders_for(&UnitKind::Unique(UnitId("URL0402".to_string())));

        assert!(
            builders.contains(&UnitKind::Engineer(TechLevel::T3)),
            "Monkeylord should be buildable by a T3 engineer"
        );
        assert!(
            !builders.contains(&UnitKind::Commander),
            "base ACU should not build Monkeylord"
        );
    }

    #[test]
    fn t1_factory_is_built_by_commander_and_t1_engineer() {
        let units = load_units();

        let builders = units.builders_for(&UnitKind::Factory(TechLevel::T1));
        assert!(builders.contains(&UnitKind::Commander));
        assert!(builders.contains(&UnitKind::Engineer(TechLevel::T1)));
    }

    #[test]
    fn unknown_unit_has_no_definition() {
        let units = load_units();
        assert!(units
            .def(&UnitKind::Unique(UnitId("NOT_A_UNIT".to_string())))
            .is_none());
    }
}
