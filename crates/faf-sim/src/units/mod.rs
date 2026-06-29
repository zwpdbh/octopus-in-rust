//! Unified unit knowledge repository.
//!
//! `Units` is a strongly-typed, self-contained model of the units that matter
//! for build-order optimization. It is built once from the raw `faf-units`
//! index and then used without string lookups by the simulator and planners.
//!
//! The model deliberately abstracts away faction-specific names for common
//! economic and builder units. A T1 engineer is just `UnitKind::Engineer(T1)`,
//! regardless of whether the raw blueprint is `UEL0105`, `URL0105`, or
//! `UAL0105`. Faction-unique units (e.g., the Monkeylord) are represented as
//! `UnitKind::Unique(UnitId)`.
//!
//! Module layout:
//!
//! - `kind` — typed unit kinds, definitions, and economy trait impls.
//! - `build` — helpers that classify raw blueprints and build recipes.
//! - `mod` — the public `Units` repository and re-exports.

use std::collections::{HashMap, HashSet};

use faf_units::DataIndex;

use crate::planner::plan_graph::{build_plan_graph, PlanGraph, PlanGraphError};
use crate::planner::strips::{build_operators, Operator};

pub use kind::{
    BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind, UpgradeRecipe,
};

mod build;
mod kind;

/// Unified repository of unit knowledge.
///
/// `Units` is self-contained: after construction it no longer references the
/// raw `DataIndex`. All build/upgrade rules are explicit recipes rather than
/// derived string-category graphs.
#[derive(Debug, Clone)]
pub struct Units {
    defs: HashMap<UnitKind, UnitDef>,
    builds: HashMap<UnitKind, BuildRecipe>,
    upgrades: HashMap<UnitKind, Vec<UpgradeRecipe>>,
}

impl Units {
    /// Build the repository from a raw unit index.
    pub fn new(index: DataIndex) -> Self {
        Self::from_index(index)
    }

    /// Build the repository from a borrowed raw unit index.
    pub fn from_ref(index: &DataIndex) -> Self {
        Self::from_index(index.clone())
    }

    fn from_index(index: DataIndex) -> Self {
        let mut defs: HashMap<UnitKind, UnitDef> = HashMap::new();
        let mut builds = Self::hardcoded_builds();
        let upgrades = Self::hardcoded_upgrades();

        // Build a lookup from raw unit id to its abstract kind. This is only
        // needed during construction to resolve unique-unit builder options.
        let mut id_to_kind: HashMap<String, UnitKind> = HashMap::new();
        for unit in &index.units {
            if let Some(kind) = build::classify_unit(unit) {
                id_to_kind.insert(unit.id.to_ascii_uppercase(), kind.clone());
            }
        }

        // Track which common kinds have already been fixed to their canonical
        // UEF blueprint. Non-canonical duplicates are ignored once the canonical
        // definition has been stored.
        let mut canonical_kinds: HashSet<UnitKind> = HashSet::new();

        for unit in &index.units {
            let Some(mut def) = build::unit_def(unit) else {
                continue;
            };

            if build::is_common_kind(&def.kind) {
                def.faction = Faction::Common;
                let is_canonical = build::is_canonical_for_kind(unit, &def.kind);
                let already_canonical = canonical_kinds.contains(&def.kind);

                // Keep an earlier non-canonical entry only until we find the
                // canonical one; afterwards ignore further duplicates.
                if defs.contains_key(&def.kind) && !is_canonical {
                    continue;
                }

                if is_canonical {
                    canonical_kinds.insert(def.kind.clone());
                } else if already_canonical {
                    continue;
                }
            }

            defs.insert(def.kind.clone(), def);
        }

        // Synthetic definitions for capped mass extractors. These do not exist
        // as raw blueprints; they represent a T2/T3 mex surrounded by four mass
        // storages, giving +50% mass income and 2000 mass storage capacity.
        defs.insert(
            UnitKind::CapT2Mex,
            UnitDef {
                kind: UnitKind::CapT2Mex,
                faction: Faction::Common,
                display_name: "Capped T2 Mass Extractor".to_string(),
                cost: UnitCost {
                    mass: 800.0,
                    energy: 6000.0,
                    build_time: 1000.0,
                },
                build_rate: 0.0,
                mass_income: 9.0,
                energy_income: 0.0,
                maintenance_energy: 9.0,
                mass_storage: 2000.0,
                energy_storage: 0.0,
            },
        );
        defs.insert(
            UnitKind::CapT3Mex,
            UnitDef {
                kind: UnitKind::CapT3Mex,
                faction: Faction::Common,
                display_name: "Capped T3 Mass Extractor".to_string(),
                cost: UnitCost {
                    mass: 800.0,
                    energy: 6000.0,
                    build_time: 1000.0,
                },
                build_rate: 0.0,
                mass_income: 27.0,
                energy_income: 0.0,
                maintenance_energy: 54.0,
                mass_storage: 2000.0,
                energy_storage: 0.0,
            },
        );

        // Generate build recipes for unique units from the raw index.
        for unit in &index.units {
            let Some(UnitKind::Unique(id)) = build::classify_unit(unit) else {
                continue;
            };
            let kind = UnitKind::Unique(id.clone());
            if builds.contains_key(&kind) {
                continue;
            }
            let Some(recipe) = build::derive_unique_recipe(unit, &id_to_kind) else {
                continue;
            };
            builds.insert(kind, recipe);
        }

        Self {
            defs,
            builds,
            upgrades,
        }
    }

    /// Look up a unit definition by kind.
    pub fn def(&self, kind: &UnitKind) -> Option<&UnitDef> {
        self.defs.get(kind)
    }

    /// Return the underlying definition map.
    pub fn defs(&self) -> &HashMap<UnitKind, UnitDef> {
        &self.defs
    }

    /// Human-readable name for a unit kind.
    pub fn display_name(&self, kind: &UnitKind) -> String {
        self.def(kind)
            .map(|d| d.display_name.clone())
            .unwrap_or_else(|| format!("{:?}", kind))
    }

    /// Build cost for a unit kind, if it can be built at all.
    pub fn build_cost(&self, kind: &UnitKind) -> Option<UnitCost> {
        self.def(kind).map(|d| d.cost)
    }

    /// True if `builder` is one of the legal builders for `target`.
    pub fn can_build(&self, builder: &UnitKind, target: &UnitKind) -> bool {
        self.build_recipe(target)
            .map(|r| r.builder_options.contains(builder))
            .unwrap_or(false)
    }

    /// Return the legal builder kinds for a target.
    pub fn builders_for(&self, target: &UnitKind) -> Vec<UnitKind> {
        self.build_recipe(target)
            .map(|r| r.builder_options.clone())
            .unwrap_or_default()
    }

    /// Return the build recipe for a target, if any.
    pub fn build_recipe(&self, target: &UnitKind) -> Option<&BuildRecipe> {
        self.builds.get(target)
    }

    /// Return all build recipes, keyed by target unit kind.
    pub fn all_build_recipes(&self) -> &HashMap<UnitKind, BuildRecipe> {
        &self.builds
    }

    /// Return the upgrade recipes available from a source unit kind.
    pub fn upgrade_recipes(&self, from: &UnitKind) -> &[UpgradeRecipe] {
        self.upgrades.get(from).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Return all upgrade recipes, keyed by source unit kind.
    pub fn all_upgrade_recipes(&self) -> &HashMap<UnitKind, Vec<UpgradeRecipe>> {
        &self.upgrades
    }

    /// Return the first upgrade target for a unit kind, if any.
    pub fn upgrade_target(&self, from: &UnitKind) -> Option<UnitKind> {
        self.upgrade_recipes(from).first().map(|r| r.to.clone())
    }

    /// True if the unit has at least one registered upgrade target.
    pub fn is_upgradeable(&self, kind: &UnitKind) -> bool {
        !self.upgrade_recipes(kind).is_empty()
    }

    /// Return all build operators for this unit repository.
    pub fn operators(&self) -> Vec<Operator> {
        build_operators(self)
    }

    /// Build a simplified, ACU-rooted plan graph for the requested unit.
    ///
    /// The graph includes the technology chain (factories, engineers) and the
    /// economic infrastructure (mex, pgen) required to reach the goal, rooted
    /// at the ACU.
    pub fn plan_graph(&self, goal: &UnitKind) -> Result<PlanGraph, PlanGraphError> {
        build_plan_graph(self, goal)
    }

    /// Return the prerequisite chain from the starting commander to a goal.
    ///
    /// The chain is the ordered list of unit kinds that must be completed
    /// before the goal can be built, excluding the commander itself and the
    /// goal.
    pub fn prerequisite_chain(&self, goal: &UnitKind) -> Vec<UnitKind> {
        let mut chain = Vec::new();
        let mut current = goal;

        // Walk backwards through prereqs until we hit a unit with no prereq
        // (typically the commander or a T1 factory built by the commander).
        while let Some(recipe) = self.build_recipe(current) {
            if let Some(prereq) = &recipe.prereq {
                // Stop when we reach the commander; it is given at game start.
                if *prereq == UnitKind::Commander {
                    break;
                }
                chain.push(prereq.clone());
                current = prereq;
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }

    /// Hardcoded build recipes for the common economic/builder units.
    fn hardcoded_builds() -> HashMap<UnitKind, BuildRecipe> {
        let mut m: HashMap<UnitKind, BuildRecipe> = HashMap::new();

        // Commander is given at game start; it is not built.
        m.insert(
            UnitKind::Commander,
            BuildRecipe {
                target: UnitKind::Commander,
                prereq: None,
                builder_options: vec![],
            },
        );

        // Factories.
        for (tech, prereq, builders) in [
            (
                TechLevel::T1,
                None,
                vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T1)],
            ),
            (
                TechLevel::T2,
                Some(UnitKind::Factory(TechLevel::T1)),
                vec![
                    UnitKind::Commander,
                    UnitKind::Engineer(TechLevel::T2),
                    UnitKind::Engineer(TechLevel::T3),
                ],
            ),
            (
                TechLevel::T3,
                Some(UnitKind::Factory(TechLevel::T2)),
                vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T3)],
            ),
        ] {
            m.insert(
                UnitKind::Factory(tech),
                BuildRecipe {
                    target: UnitKind::Factory(tech),
                    prereq,
                    builder_options: builders,
                },
            );
        }

        // Engineers are built by factories of the same tier.
        for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
            m.insert(
                UnitKind::Engineer(tech),
                BuildRecipe {
                    target: UnitKind::Engineer(tech),
                    prereq: Some(UnitKind::Factory(tech)),
                    builder_options: vec![UnitKind::Factory(tech)],
                },
            );
        }

        // T1 economy structures are built by the commander or T1 engineers.
        for kind in [UnitKind::Mex(TechLevel::T1), UnitKind::Pgen(TechLevel::T1)] {
            m.insert(
                kind.clone(),
                BuildRecipe {
                    target: kind,
                    prereq: None,
                    builder_options: vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T1)],
                },
            );
        }

        // Higher-tier economy structures are built by engineers of the same
        // tier (new construction, not upgrade).
        for tech in [TechLevel::T2, TechLevel::T3] {
            for kind in [UnitKind::Mex(tech), UnitKind::Pgen(tech)] {
                m.insert(
                    kind.clone(),
                    BuildRecipe {
                        target: kind.clone(),
                        prereq: Some(UnitKind::Factory(tech)),
                        builder_options: vec![UnitKind::Engineer(tech)],
                    },
                );
            }
        }

        // Energy storage can be built by any engineer tier once an engineer exists.
        m.insert(
            UnitKind::EnergyStorage,
            BuildRecipe {
                target: UnitKind::EnergyStorage,
                prereq: None,
                builder_options: vec![
                    UnitKind::Engineer(TechLevel::T1),
                    UnitKind::Engineer(TechLevel::T2),
                    UnitKind::Engineer(TechLevel::T3),
                ],
            },
        );

        m
    }

    /// Hardcoded upgrade recipes for the common economic units.
    fn hardcoded_upgrades() -> HashMap<UnitKind, Vec<UpgradeRecipe>> {
        let mut m: HashMap<UnitKind, Vec<UpgradeRecipe>> = HashMap::new();

        let any_engineer = vec![
            UnitKind::Commander,
            UnitKind::Engineer(TechLevel::T1),
            UnitKind::Engineer(TechLevel::T2),
            UnitKind::Engineer(TechLevel::T3),
        ];
        let t2_plus_engineer = vec![
            UnitKind::Commander,
            UnitKind::Engineer(TechLevel::T2),
            UnitKind::Engineer(TechLevel::T3),
        ];

        // Mass extractors: T1 -> T2 -> T3, plus capped variants.
        m.insert(
            UnitKind::Mex(TechLevel::T1),
            vec![UpgradeRecipe {
                from: UnitKind::Mex(TechLevel::T1),
                to: UnitKind::Mex(TechLevel::T2),
                cost: UnitCost {
                    mass: 900.0,
                    energy: 5400.0,
                    build_time: 900.0,
                },
                builder_options: any_engineer.clone(),
            }],
        );
        m.insert(
            UnitKind::Mex(TechLevel::T2),
            vec![
                UpgradeRecipe {
                    from: UnitKind::Mex(TechLevel::T2),
                    to: UnitKind::Mex(TechLevel::T3),
                    cost: UnitCost {
                        mass: 4600.0,
                        energy: 31625.0,
                        build_time: 6000.0,
                    },
                    builder_options: t2_plus_engineer.clone(),
                },
                // Cap a T2 mex with four mass storages.
                UpgradeRecipe {
                    from: UnitKind::Mex(TechLevel::T2),
                    to: UnitKind::CapT2Mex,
                    cost: UnitCost {
                        mass: 800.0,
                        energy: 6000.0,
                        build_time: 1000.0,
                    },
                    builder_options: any_engineer.clone(),
                },
            ],
        );
        m.insert(
            UnitKind::Mex(TechLevel::T3),
            vec![UpgradeRecipe {
                from: UnitKind::Mex(TechLevel::T3),
                to: UnitKind::CapT3Mex,
                cost: UnitCost {
                    mass: 800.0,
                    energy: 6000.0,
                    build_time: 1000.0,
                },
                builder_options: t2_plus_engineer.clone(),
            }],
        );
        m.insert(
            UnitKind::CapT2Mex,
            vec![UpgradeRecipe {
                from: UnitKind::CapT2Mex,
                to: UnitKind::CapT3Mex,
                cost: UnitCost {
                    mass: 4600.0,
                    energy: 31625.0,
                    build_time: 6000.0,
                },
                builder_options: t2_plus_engineer.clone(),
            }],
        );

        // Power generators: T1 -> T2 -> T3.
        m.insert(
            UnitKind::Pgen(TechLevel::T1),
            vec![UpgradeRecipe {
                from: UnitKind::Pgen(TechLevel::T1),
                to: UnitKind::Pgen(TechLevel::T2),
                cost: UnitCost {
                    mass: 1200.0,
                    energy: 8000.0,
                    build_time: 1500.0,
                },
                builder_options: any_engineer.clone(),
            }],
        );
        m.insert(
            UnitKind::Pgen(TechLevel::T2),
            vec![UpgradeRecipe {
                from: UnitKind::Pgen(TechLevel::T2),
                to: UnitKind::Pgen(TechLevel::T3),
                cost: UnitCost {
                    mass: 3240.0,
                    energy: 40000.0,
                    build_time: 3000.0,
                },
                builder_options: t2_plus_engineer.clone(),
            }],
        );

        // Factories: T1 -> T2 -> T3.
        m.insert(
            UnitKind::Factory(TechLevel::T1),
            vec![UpgradeRecipe {
                from: UnitKind::Factory(TechLevel::T1),
                to: UnitKind::Factory(TechLevel::T2),
                cost: UnitCost {
                    mass: 800.0,
                    energy: 4800.0,
                    build_time: 800.0,
                },
                builder_options: any_engineer.clone(),
            }],
        );
        m.insert(
            UnitKind::Factory(TechLevel::T2),
            vec![UpgradeRecipe {
                from: UnitKind::Factory(TechLevel::T2),
                to: UnitKind::Factory(TechLevel::T3),
                cost: UnitCost {
                    mass: 2400.0,
                    energy: 22000.0,
                    build_time: 2400.0,
                },
                // Engineer(T2) is allowed so the search can reach T3 without
                // already owning a T3 engineer (the engineering-suite
                // prerequisite is abstracted away in this model).
                builder_options: t2_plus_engineer.clone(),
            }],
        );

        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn units_answers_build_and_upgrade_questions() {
        let units = load_units();

        // Build questions.
        assert!(units.can_build(&UnitKind::Commander, &UnitKind::Pgen(TechLevel::T1)));
        assert!(units.can_build(
            &UnitKind::Engineer(TechLevel::T1),
            &UnitKind::Pgen(TechLevel::T1)
        ));
        assert!(!units.can_build(&UnitKind::Commander, &UnitKind::Engineer(TechLevel::T3)));

        // Upgrade questions.
        assert!(units.is_upgradeable(&UnitKind::Mex(TechLevel::T1)));
        assert_eq!(
            units.upgrade_target(&UnitKind::Mex(TechLevel::T1)),
            Some(UnitKind::Mex(TechLevel::T2))
        );

        // Non-upgradeable unit.
        assert!(!units.is_upgradeable(&UnitKind::Engineer(TechLevel::T1)));
        assert!(units
            .upgrade_target(&UnitKind::Engineer(TechLevel::T1))
            .is_none());
    }

    #[test]
    fn storage_and_capped_mex_units_are_defined() {
        let units = load_units();

        let cap_t2_def = units.def(&UnitKind::CapT2Mex).expect("capped t2 mex def");
        assert_eq!(cap_t2_def.kind, UnitKind::CapT2Mex);
        assert!(cap_t2_def.mass_storage > 0.0);
        assert!(cap_t2_def.mass_income > 6.0); // boosted by adjacency

        let cap_t3_def = units.def(&UnitKind::CapT3Mex).expect("capped t3 mex def");
        assert_eq!(cap_t3_def.kind, UnitKind::CapT3Mex);
        assert!(cap_t3_def.mass_storage > 0.0);
        assert!(cap_t3_def.mass_income > 18.0); // boosted by adjacency

        let energy_storage_def = units
            .def(&UnitKind::EnergyStorage)
            .expect("energy storage def");
        assert_eq!(energy_storage_def.kind, UnitKind::EnergyStorage);
        assert!(energy_storage_def.energy_storage > 0.0);

        assert!(units.can_build(&UnitKind::Engineer(TechLevel::T1), &UnitKind::EnergyStorage));
        assert!(units.is_upgradeable(&UnitKind::Mex(TechLevel::T2)));
        assert!(units
            .upgrade_recipes(&UnitKind::Mex(TechLevel::T2))
            .iter()
            .any(|r| r.to == UnitKind::CapT2Mex));
        assert!(units
            .upgrade_recipes(&UnitKind::CapT2Mex)
            .iter()
            .any(|r| r.to == UnitKind::CapT3Mex));
    }
}
