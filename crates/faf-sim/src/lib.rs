//! Simulation and build-order planning for Forged Alliance Forever (FAF).
//!
//! This crate sits on top of `faf-units` and provides:
//!
//! - `economy` — continuous-drain resource model.
//! - `sim` — economy derivation and graph-growth simulator.
//! - `planner` — planner trait, strategy registry, and concrete planner
//!   implementations.
//! - `units` — unified unit knowledge repository (unit kinds, recipes, stats).

pub mod decision_actor;
pub mod economy;
pub mod message;
pub mod planner;
pub mod sim;
pub mod sim_actor;
pub mod units;

pub use decision_actor::DecisionActor;
pub use economy::{
    apply_tick, apply_tick_graph, compute_drain, total_build_power, BuildDrain, BuildProject,
    EcoFlow, EconomyState, EffectiveBuildPower, GraphTickResult, RequestedBuildPower,
    ResourceProducer, TickOutcome, TickResult,
};
pub use message::{Command, Observation};
pub use planner::{
    build_operators, build_plan_graph, Fact, Operator, PlanEdgeKind, PlanGraph, PlanGraphError,
    PlanResult, Planner, PlannerConfig, PlannerError, Strategy, StripsAction, ValueNetKind,
};
pub use sim::{
    derive_economy, run_build_order_simulation, BuildEdge, BuildEvent, BuildGraph, GraphSimError,
    GraphState, NodeId, OngoingBuild, SimulationConfig, SimulationError, SimulationResult,
    UnitNode,
};
pub use sim_actor::SimActor;

pub use units::{
    BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind, Units, UpgradeRecipe,
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
    fn monkeylord_prerequisites_contain_factory_and_engineer_chains() {
        let units = load_units();

        let prereqs = units.prerequisite_chain(&UnitKind::Unique(UnitId("URL0402".to_string())));

        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T1)),
            "T1 factory"
        );
        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T2)),
            "T2 factory"
        );
        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T3)),
            "T3 factory"
        );

        // Commanders are the default stopping point, so it is not expanded.
        assert!(!prereqs.contains(&UnitKind::Commander));
    }

    #[test]
    fn fatboy_prerequisites_contain_factory_and_engineer_chains() {
        let units = load_units();

        let direct = units.builders_for(&UnitKind::Unique(UnitId("UEL0401".to_string())));
        assert!(
            direct.contains(&UnitKind::Engineer(TechLevel::T3)),
            "Fatboy should be buildable by a T3 engineer"
        );
        assert!(
            !direct.contains(&UnitKind::Commander),
            "base ACU should not build Fatboy"
        );

        let prereqs = units.prerequisite_chain(&UnitKind::Unique(UnitId("UEL0401".to_string())));
        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T1)),
            "T1 factory"
        );
        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T2)),
            "T2 factory"
        );
        assert!(
            prereqs.contains(&UnitKind::Factory(TechLevel::T3)),
            "T3 factory"
        );

        // ACU is the default stop point, so it should not appear as a prerequisite.
        assert!(!prereqs.contains(&UnitKind::Commander));
    }

    #[test]
    fn unknown_unit_has_no_definition() {
        let units = load_units();
        assert!(units
            .def(&UnitKind::Unique(UnitId("NOT_A_UNIT".to_string())))
            .is_none());
    }
}
