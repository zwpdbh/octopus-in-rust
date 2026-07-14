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

pub use category::{category_of, UnitCategory};
pub use kind::{
    BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind, UnitRole, UpgradeRecipe,
};

mod build;
mod category;
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
        // storages. The base `ProductionPerSecondMass` matches the underlying mex; the +50%
        // adjacency bonus is applied at runtime by the adjacency tracker.
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
                role: UnitRole::CappedMassExtractor {
                    production_per_second_mass: 6.0,
                    mass_storage: 2000.0,
                    maintenance_consumption_per_second_energy: 9.0,
                },
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
                role: UnitRole::CappedMassExtractor {
                    production_per_second_mass: 18.0,
                    mass_storage: 2000.0,
                    maintenance_consumption_per_second_energy: 54.0,
                },
            },
        );

        // Faction-unique units (experimentals, game-enders, strategic weapons) are
        // built by T3 engineers. Derive their build recipes from the loaded defs.
        for kind in defs.keys() {
            if matches!(kind, UnitKind::Unique(_)) {
                builds.insert(
                    kind.clone(),
                    BuildRecipe {
                        target: kind.clone(),
                        prereq: None,
                        builder_options: vec![UnitKind::Engineer(TechLevel::T3)],
                    },
                );
            }
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

    /// Blueprint id for a unit kind, if one exists.
    ///
    /// Common units map to their canonical UEF representative; unique units return
    /// their own blueprint id.
    pub fn blueprint_id(&self, kind: &UnitKind) -> Option<String> {
        match kind {
            UnitKind::Unique(id) => Some(id.0.clone()),
            UnitKind::CapT2Mex => Some("UEB1202".to_string()),
            UnitKind::CapT3Mex => Some("UEB1302".to_string()),
            _ => build::canonical_blueprint_id(kind).map(|s| s.to_string()),
        }
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

    /// Return every unit kind that `builder` is allowed to build.
    pub fn buildable_by(&self, builder: &UnitKind) -> Vec<UnitKind> {
        self.builds
            .values()
            .filter(|r| r.builder_options.contains(builder))
            .map(|r| r.target.clone())
            .collect()
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
    fn commander_builds_t1_economy_and_factories() {
        let units = load_units();

        let buildable = units.buildable_by(&UnitKind::Commander);
        assert!(buildable.contains(&UnitKind::Mex(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(!buildable.contains(&UnitKind::Engineer(TechLevel::T1)));
    }

    #[test]
    fn factory_builds_its_tier_engineers() {
        let units = load_units();

        let t1 = units.buildable_by(&UnitKind::Factory(TechLevel::T1));
        assert!(t1.contains(&UnitKind::Engineer(TechLevel::T1)));
        assert!(!t1.contains(&UnitKind::Engineer(TechLevel::T2)));

        let t2 = units.buildable_by(&UnitKind::Factory(TechLevel::T2));
        assert!(t2.contains(&UnitKind::Engineer(TechLevel::T2)));
    }

    #[test]
    fn categories_group_common_units() {
        use super::category::category_of;
        use super::category::UnitCategory;

        assert_eq!(category_of(&UnitKind::Commander), UnitCategory::Commander);
        assert_eq!(
            category_of(&UnitKind::Engineer(TechLevel::T1)),
            UnitCategory::Engineer
        );
        assert_eq!(
            category_of(&UnitKind::Factory(TechLevel::T1)),
            UnitCategory::Factory
        );
        assert_eq!(
            category_of(&UnitKind::Mex(TechLevel::T1)),
            UnitCategory::Economic
        );
        assert_eq!(
            category_of(&UnitKind::Pgen(TechLevel::T1)),
            UnitCategory::Economic
        );
    }

    #[test]
    fn storage_and_capped_mex_units_are_defined() {
        let units = load_units();

        let t2_mex_def = units.def(&UnitKind::Mex(TechLevel::T2)).unwrap();
        let t3_mex_def = units.def(&UnitKind::Mex(TechLevel::T3)).unwrap();

        // Capped mexes share the base income of the underlying mex; the +50%
        // adjacency bonus is applied at runtime by the adjacency tracker.
        let cap_t2_def = units.def(&UnitKind::CapT2Mex).expect("capped t2 mex def");
        assert_eq!(cap_t2_def.kind, UnitKind::CapT2Mex);
        assert!(cap_t2_def.mass_storage() > 0.0);
        assert!(
            (cap_t2_def.production_per_second_mass() - t2_mex_def.production_per_second_mass())
                .abs()
                < 1e-9
        );

        let cap_t3_def = units.def(&UnitKind::CapT3Mex).expect("capped t3 mex def");
        assert_eq!(cap_t3_def.kind, UnitKind::CapT3Mex);
        assert!(cap_t3_def.mass_storage() > 0.0);
        assert!(
            (cap_t3_def.production_per_second_mass() - t3_mex_def.production_per_second_mass())
                .abs()
                < 1e-9
        );

        let energy_storage_def = units
            .def(&UnitKind::EnergyStorage)
            .expect("energy storage def");
        assert_eq!(energy_storage_def.kind, UnitKind::EnergyStorage);
        assert!(energy_storage_def.energy_storage() > 0.0);

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
