//! Headless, observable eco/build simulator for Forged Alliance Forever (FAF).
//!
//! The crate provides:
//!
//! - `economy`: pure math for drains, stalls, and resource tracking.
//! - `units`: ECS-backed blueprint library (`BlueprintLibrary`) and unit kinds.
//! - `runtime`: an observable, steppable Bevy ECS economy simulation.
//! - `sim`: the high-level simulation driver and re-exports.
//!
//! Core queue/economy types are re-exported from `faf-sim-shared`.

pub mod economy;
pub mod protocol;
pub mod quantities;
pub mod runtime;
pub mod sim;
pub mod snapshot;
pub use faf_blueprints as units;

pub use economy::{
    apply_tick, apply_tick_graph, compute_drain, total_build_power, BuildDrain, BuildProject,
    EcoFlow, EconomyRuntimeState, EffectiveBuildPower, GraphTickResult, RequestedBuildPower,
    ResourceProducer, TickOutcome, TickResult,
};
pub use quantities::{BuildPower, BuildWork, Energy, EnergyRate, Mass, MassRate, Storage, Time};
pub use runtime::{
    BuildQueue, BuildQueueSimulationPlugin, BuildTask, EcoSnapshot, SimulationEvent, UnitEcoStats,
};
pub use snapshot::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};
pub use units::{
    category_of, category_of_role, role_of, tech_level_of, BlueprintEdge, BlueprintGraph,
    BlueprintLibrary, BlueprintNode, BuildRule, BuiltBy, DisplayName, Faction, FactionComp,
    TechLevel, TechLevelComp, UnitCategory, UnitCost, UnitId, UnitKind, UnitKindComp, UnitRole,
    UnitRoleComp, UpgradePath, UpgradesInto,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn load_library() -> BlueprintLibrary {
        BlueprintLibrary::from_default_units().expect("default units should load")
    }

    #[test]
    fn monkeylord_requires_t3_engineer() {
        let units = load_library();

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
        let units = load_library();

        let builders = units.builders_for(&UnitKind::Factory(TechLevel::T1));
        assert!(builders.contains(&UnitKind::Commander));
        assert!(builders.contains(&UnitKind::Engineer(TechLevel::T1)));
    }

    #[test]
    fn unknown_unit_has_no_definition() {
        let units = load_library();
        assert!(units
            .entity_for_kind(&UnitKind::Unique(UnitId("NOT_A_UNIT".to_string())))
            .is_none());
    }
}
