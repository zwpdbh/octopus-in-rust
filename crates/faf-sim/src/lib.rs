//! Headless, observable eco/build simulator for Forged Alliance Forever (FAF).
//!
//! The crate provides:
//!
//! - `economy`: pure math for drains, stalls, and resource tracking.
//! - `units`: strongly-typed unit knowledge (`Units`, `UnitKind`, recipes).
//! - `runtime`: an observable, steppable Bevy ECS economy simulation.
//! - `sim`: the high-level simulation driver and re-exports.

pub mod economy;
pub mod protocol;
pub mod quantities;
pub mod runtime;
pub mod sim;
pub mod snapshot;
pub mod units;

pub use economy::{
    apply_tick, apply_tick_graph, compute_drain, total_build_power, BuildDrain, BuildProject,
    EcoFlow, EconomyRuntimeState, EffectiveBuildPower, GraphTickResult, RequestedBuildPower,
    ResourceProducer, TickOutcome, TickResult,
};
pub use quantities::{BuildPower, BuildWork, Energy, EnergyRate, Mass, MassRate, Storage, Time};
pub use runtime::{BuildQueue, BuildTask, EcoPlugin, EcoSnapshot, SimulationEvent, UnitEcoStats};
pub use snapshot::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};
pub use units::{
    BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind, UnitRole, Units,
    UpgradeRecipe,
};

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
