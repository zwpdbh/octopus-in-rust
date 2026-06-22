//! Simulation and build-order planning for Forged Alliance Forever (FAF).
//!
//! This crate sits on top of `faf-units` and provides:
//!
//! - `build_graph` — pure unit dependency graph (who can build whom).
//! - `economy` — continuous-drain resource model.
//! - `sim` — economy and build-time simulation.
//! - `heuristic` — greedy build-order planner that grows BP while avoiding stalls.

pub mod build_graph;
pub mod economy;
pub mod heuristic;
pub mod sim;

pub use build_graph::{BuildGraph, BuilderKind, UnknownUnitError};
pub use economy::{
    apply_tick, compute_drain, total_build_power, BuildDrain, BuildProject, EconomyState,
    EffectiveBuildPower, RequestedBuildPower, TickOutcome, TickResult,
};
pub use heuristic::{
    BuildPolicy, HeuristicSimulator, ProductionFocus, ProjectPriority, ProjectRequest,
    StateMachinePolicy,
};
pub use sim::{derive_economy, BuildEvent};

#[cfg(test)]
mod tests {
    use super::*;
    use faf_units::DataIndex;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn monkeylord_has_t3_engineer_and_commander_prerequisites() {
        let index = load_index();
        let graph = BuildGraph::new(&index);

        let builders: Vec<String> = graph
            .builders_for("URL0402")
            .into_iter()
            .map(|u| u.id.clone())
            .collect();

        assert!(
            builders.contains(&"URL0001".to_string()),
            "Monkeylord should be buildable by the Cybran ACU (COMMAND)"
        );
        assert!(
            builders.contains(&"URL0309".to_string()),
            "Monkeylord should be buildable by the Cybran T3 engineer"
        );
    }

    #[test]
    fn t1_factory_is_built_by_commander_and_t1_engineer() {
        let index = load_index();
        let graph = BuildGraph::new(&index);

        let builders: Vec<String> = graph
            .builders_for("URB0101")
            .into_iter()
            .map(|u| u.id.clone())
            .collect();

        assert!(builders.contains(&"URL0001".to_string()));
        assert!(builders.contains(&"URL0105".to_string()));
    }

    #[test]
    fn all_prerequisites_for_monkeylord_via_engineer_path() {
        let index = load_index();
        let graph = BuildGraph::new(&index);

        let prereqs: Vec<String> = graph
            .all_prerequisites_default("URL0402")
            .expect("URL0402 exists")
            .into_iter()
            .map(|u| u.id.clone())
            .collect();

        // Via the T3 engineer path, we need the full factory upgrade chain.
        assert!(prereqs.contains(&"URL0309".to_string()), "T3 engineer");
        assert!(prereqs.contains(&"URB0301".to_string()), "T3 factory");
        assert!(prereqs.contains(&"URB0201".to_string()), "T2 factory");
        assert!(prereqs.contains(&"URB0101".to_string()), "T1 factory");

        // Commanders are the default stopping point, so URL0001 is not expanded.
        assert!(!prereqs.contains(&"URL0001".to_string()));
    }

    #[test]
    fn uef_fatboy_prerequisites_contain_factory_and_engineer_chains() {
        let index = load_index();
        let graph = BuildGraph::new(&index);

        let direct: Vec<String> = graph
            .builders_for("UEL0401")
            .into_iter()
            .map(|u| u.id.clone())
            .collect();
        assert!(
            direct.contains(&"UEL0309".to_string()),
            "Fatboy should be buildable by UEF T3 engineer"
        );
        assert!(
            direct.contains(&"UEL0001".to_string()),
            "Fatboy should be buildable by UEF ACU"
        );
        // Only UEF builders.
        assert!(
            direct
                .iter()
                .all(|id| id.starts_with("UE") || id.starts_with("XEL")),
            "all direct builders should be UEF: {:?}",
            direct
        );

        let prereqs: Vec<String> = graph
            .all_prerequisites_default("UEL0401")
            .expect("UEL0401 exists")
            .into_iter()
            .map(|u| u.id.clone())
            .collect();

        // Factory upgrade chain.
        assert!(
            prereqs.contains(&"UEB0301".to_string())
                || prereqs.contains(&"UEB0302".to_string())
                || prereqs.contains(&"UEB0303".to_string())
                || prereqs.contains(&"UEB0304".to_string()),
            "T3 factory prerequisite"
        );
        assert!(
            prereqs.contains(&"UEB0201".to_string())
                || prereqs.contains(&"UEB0202".to_string())
                || prereqs.contains(&"UEB0203".to_string()),
            "T2 factory prerequisite"
        );
        assert!(
            prereqs.contains(&"UEB0101".to_string())
                || prereqs.contains(&"UEB0102".to_string())
                || prereqs.contains(&"UEB0103".to_string()),
            "T1 factory prerequisite"
        );

        // T3 engineer prerequisite.
        assert!(
            prereqs.contains(&"UEL0309".to_string()),
            "T3 engineer prerequisite"
        );

        // ACU is the default stop point, so it should not appear as a prerequisite.
        assert!(!prereqs.contains(&"UEL0001".to_string()));
    }

    #[test]
    fn unknown_unit_returns_error() {
        let index = load_index();
        let graph = BuildGraph::new(&index);
        assert!(graph.all_prerequisites_default("NOT_A_UNIT").is_err());
    }
}
