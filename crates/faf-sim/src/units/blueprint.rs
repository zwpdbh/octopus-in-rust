//! ECS-backed blueprint library for unit knowledge.
//!
//! [`BlueprintLibrary`] is a self-contained, ECS-backed model of the units that
//! matter for build-order optimization. It is built once from the raw
//! `faf-units` index and then used without string lookups by the simulator and
//! planners.
//!
//! Each unit definition is represented as a blueprint entity in a dedicated
//! Bevy `World`. Static attributes are stored as components; see the
//! `components` module for the full list.

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use faf_units::DataIndex;

use crate::runtime::UnitEcoStats;

use super::build;
use super::components::{
    BlueprintBundle, BlueprintId, BuildPower, BuildRecipeComp, DisplayName, EconomyProfile,
    FactionComp, StorageProfile, UnitCostComp, UnitKindComp, UnitRoleComp, UpgradeRecipesComp,
};
use super::types::{
    category_of, matches_tech_level, role_of, BuildRecipe, Faction, TechLevel, UnitCategory,
    UnitCost, UnitKind, UnitRole, UpgradeRecipe,
};

/// Unified repository of unit knowledge backed by a Bevy ECS blueprint world.
///
/// `BlueprintLibrary` is self-contained: after construction it no longer
/// references the raw `DataIndex`. All build/upgrade rules are explicit recipes
/// rather than derived string-category graphs.
#[derive(Debug)]
pub struct BlueprintLibrary {
    world: World,
    kind_to_entity: HashMap<UnitKind, Entity>,
}

impl BlueprintLibrary {
    /// Build the repository from a raw unit index.
    pub fn new(index: DataIndex) -> Self {
        Self::from_index(index)
    }

    /// Build the repository from a borrowed raw unit index.
    pub fn from_ref(index: &DataIndex) -> Self {
        Self::from_index(index.clone())
    }

    fn from_index(index: DataIndex) -> Self {
        let mut world = World::new();
        let mut kind_to_entity: HashMap<UnitKind, Entity> = HashMap::new();
        let builds = Self::hardcoded_builds();
        let upgrades = Self::hardcoded_upgrades();

        // Track which common kinds have already been fixed to their canonical
        // UEF blueprint. Non-canonical duplicates are ignored once the canonical
        // definition has been stored.
        let mut canonical_kinds: HashSet<UnitKind> = HashSet::new();

        for unit in &index.units {
            let Some(bundle) = build::blueprint_bundle(unit) else {
                continue;
            };
            let kind = bundle.kind.0.clone();

            if build::is_common_kind(&kind) {
                let is_canonical = build::is_canonical_for_kind(unit, &kind);
                let already_canonical = canonical_kinds.contains(&kind);

                // Keep an earlier non-canonical entry only until we find the
                // canonical one; afterwards ignore further duplicates.
                if kind_to_entity.contains_key(&kind) && !is_canonical {
                    continue;
                }

                if is_canonical {
                    canonical_kinds.insert(kind.clone());
                } else if already_canonical {
                    continue;
                }
            }

            let entity = world.spawn(bundle).id();
            kind_to_entity.insert(kind, entity);
        }

        // Synthetic definitions for capped mass extractors. These do not exist
        // as raw blueprints; they represent a T2/T3 mex surrounded by four mass
        // storages. The base `ProductionPerSecondMass` matches the underlying
        // mex; the +50% adjacency bonus is applied at runtime by the adjacency
        // tracker.
        let cap_t2_entity = world
            .spawn(BlueprintBundle {
                blueprint_id: BlueprintId("UEB1202+CAPPED".to_string()),
                kind: UnitKindComp(UnitKind::CapT2Mex),
                role: UnitRoleComp(UnitRole::CappedMassExtractor),
                faction: FactionComp(Faction::Common),
                display_name: DisplayName("Capped T2 Mass Extractor".to_string()),
                cost: UnitCostComp(UnitCost {
                    mass: 800.0,
                    energy: 6000.0,
                    build_time: 1000.0,
                }),
                build_power: BuildPower(0.0),
                economy: EconomyProfile {
                    production_per_second_mass: 6.0,
                    production_per_second_energy: 0.0,
                    maintenance_consumption_per_second_energy: 9.0,
                },
                storage: StorageProfile {
                    mass: 2000.0,
                    energy: 0.0,
                },
            })
            .id();
        kind_to_entity.insert(UnitKind::CapT2Mex, cap_t2_entity);

        let cap_t3_entity = world
            .spawn(BlueprintBundle {
                blueprint_id: BlueprintId("UEB1302+CAPPED".to_string()),
                kind: UnitKindComp(UnitKind::CapT3Mex),
                role: UnitRoleComp(UnitRole::CappedMassExtractor),
                faction: FactionComp(Faction::Common),
                display_name: DisplayName("Capped T3 Mass Extractor".to_string()),
                cost: UnitCostComp(UnitCost {
                    mass: 800.0,
                    energy: 6000.0,
                    build_time: 1000.0,
                }),
                build_power: BuildPower(0.0),
                economy: EconomyProfile {
                    production_per_second_mass: 18.0,
                    production_per_second_energy: 0.0,
                    maintenance_consumption_per_second_energy: 54.0,
                },
                storage: StorageProfile {
                    mass: 2000.0,
                    energy: 0.0,
                },
            })
            .id();
        kind_to_entity.insert(UnitKind::CapT3Mex, cap_t3_entity);

        // Attach build and upgrade recipes to their target/source entities.
        for (target, recipe) in &builds {
            if let Some(&entity) = kind_to_entity.get(target) {
                world.entity_mut(entity).insert(BuildRecipeComp {
                    prereq: recipe.prereq.clone(),
                    builder_options: recipe.builder_options.clone(),
                });
            }
        }
        for (from, recipes) in &upgrades {
            if let Some(&entity) = kind_to_entity.get(from) {
                world
                    .entity_mut(entity)
                    .insert(UpgradeRecipesComp(recipes.clone()));
            }
        }

        // Faction-unique units (experimentals, game-enders, strategic weapons)
        // are built by T3 engineers. Derive their build recipes from the loaded
        // blueprint entities.
        for (kind, &entity) in &kind_to_entity {
            if matches!(kind, UnitKind::Unique(_)) {
                world.entity_mut(entity).insert(BuildRecipeComp {
                    prereq: None,
                    builder_options: vec![UnitKind::Engineer(TechLevel::T3)],
                });
            }
        }

        Self {
            world,
            kind_to_entity,
        }
    }

    /// Entity handle for the blueprint of `kind`, if one exists.
    pub fn entity_for_kind(&self, kind: &UnitKind) -> Option<Entity> {
        self.kind_to_entity.get(kind).copied()
    }

    /// Functional role for a unit kind.
    pub fn role(&self, kind: &UnitKind) -> UnitRole {
        role_of(kind)
    }

    /// UI category for a unit kind.
    pub fn category(&self, kind: &UnitKind) -> UnitCategory {
        category_of(kind)
    }

    /// All unit kinds known to the library.
    pub fn all_kinds(&self) -> Vec<UnitKind> {
        let mut kinds: Vec<UnitKind> = self.kind_to_entity.keys().cloned().collect();
        kinds.sort();
        kinds
    }

    /// All unit kinds whose abstract classification matches the given tech level.
    ///
    /// For example, `kinds_with_tech_level(TechLevel::T2)` returns T2 engineers,
    /// factories, mass extractors, and power generators.
    pub fn kinds_with_tech_level(&self, tech: TechLevel) -> Vec<UnitKind> {
        self.kind_to_entity
            .keys()
            .filter(|kind| matches_tech_level(kind, tech))
            .cloned()
            .collect()
    }

    /// Blueprint id for a unit kind, if one exists.
    ///
    /// Common units map to their canonical UEF representative; unique units
    /// return their own blueprint id.
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
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<DisplayName>())
            .map(|d| d.0.clone())
            .unwrap_or_else(|| format!("{:?}", kind))
    }

    /// Build cost for a unit kind, if it can be built at all.
    pub fn build_cost(&self, kind: &UnitKind) -> Option<UnitCost> {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<UnitCostComp>())
            .map(|c| (*c).into())
    }

    /// True if `builder` is one of the legal builders for `target`.
    pub fn can_build(&self, builder: &UnitKind, target: &UnitKind) -> bool {
        self.build_recipe(target)
            .map(|r| r.builder_options.contains(builder))
            .unwrap_or(false)
    }

    /// Return every unit kind that `builder` is allowed to build.
    pub fn buildable_by(&self, builder: &UnitKind) -> Vec<UnitKind> {
        self.buildable_targets()
            .into_iter()
            .filter(|(_, recipe)| recipe.builder_options.contains(builder))
            .map(|(kind, _)| kind)
            .collect()
    }

    /// Return the legal builder kinds for a target.
    pub fn builders_for(&self, target: &UnitKind) -> Vec<UnitKind> {
        self.build_recipe(target)
            .map(|r| r.builder_options.clone())
            .unwrap_or_default()
    }

    /// Return the build recipe for a target, if any.
    pub fn build_recipe(&self, target: &UnitKind) -> Option<&BuildRecipeComp> {
        let entity = self.entity_for_kind(target)?;
        self.world.entity(entity).get::<BuildRecipeComp>()
    }

    /// Return all buildable target kinds with their recipes.
    pub fn buildable_targets(&self) -> Vec<(UnitKind, &BuildRecipeComp)> {
        let mut targets: Vec<(UnitKind, &BuildRecipeComp)> = self
            .kind_to_entity
            .iter()
            .filter_map(|(kind, &entity)| {
                self.world
                    .entity(entity)
                    .get::<BuildRecipeComp>()
                    .map(|recipe| (kind.clone(), recipe))
            })
            .collect();
        targets.sort_by(|a, b| a.0.cmp(&b.0));
        targets
    }

    /// Return the upgrade recipes available from a source unit kind.
    pub fn upgrade_recipes(&self, from: &UnitKind) -> &[UpgradeRecipe] {
        self.entity_for_kind(from)
            .and_then(|e| self.world.entity(e).get::<UpgradeRecipesComp>())
            .map(|r| r.0.as_slice())
            .unwrap_or(&[])
    }

    /// Return the first upgrade target for a unit kind, if any.
    pub fn upgrade_target(&self, from: &UnitKind) -> Option<UnitKind> {
        self.upgrade_recipes(from).first().map(|r| r.to.clone())
    }

    /// True if the unit has at least one registered upgrade target.
    pub fn is_upgradeable(&self, kind: &UnitKind) -> bool {
        !self.upgrade_recipes(kind).is_empty()
    }

    /// All blueprint entities that have positive build power.
    pub fn builders(&self) -> Vec<(UnitKind, BuildPower)> {
        let mut builders: Vec<(UnitKind, BuildPower)> = self
            .kind_to_entity
            .iter()
            .filter_map(|(kind, &entity)| {
                self.world
                    .entity(entity)
                    .get::<BuildPower>()
                    .filter(|bp| bp.0 > 0.0)
                    .map(|bp| (kind.clone(), *bp))
            })
            .collect();
        builders.sort_by(|a, b| a.0.cmp(&b.0));
        builders
    }

    /// Build power for a unit kind.
    pub fn build_power(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<BuildPower>())
            .map(|bp| bp.0)
            .unwrap_or(0.0)
    }

    /// Mass production per second for a unit kind.
    pub fn production_per_second_mass(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<EconomyProfile>())
            .map(|ep| ep.production_per_second_mass)
            .unwrap_or(0.0)
    }

    /// Energy production per second for a unit kind.
    pub fn production_per_second_energy(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<EconomyProfile>())
            .map(|ep| ep.production_per_second_energy)
            .unwrap_or(0.0)
    }

    /// Energy maintenance consumption per second for a unit kind.
    pub fn maintenance_consumption_per_second_energy(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<EconomyProfile>())
            .map(|ep| ep.maintenance_consumption_per_second_energy)
            .unwrap_or(0.0)
    }

    /// Mass storage capacity for a unit kind.
    pub fn mass_storage(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<StorageProfile>())
            .map(|sp| sp.mass)
            .unwrap_or(0.0)
    }

    /// Energy storage capacity for a unit kind.
    pub fn energy_storage(&self, kind: &UnitKind) -> f64 {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<StorageProfile>())
            .map(|sp| sp.energy)
            .unwrap_or(0.0)
    }

    /// Convert a blueprint into the flat runtime economic representation.
    ///
    /// `as_builder` controls whether cost/storage fields are zeroed out, matching
    /// the old `unit_as_builder` / `unit_as_target` split.
    pub fn to_unit_eco_stats(&self, kind: &UnitKind, as_builder: bool) -> Option<UnitEcoStats> {
        let entity = self.entity_for_kind(kind)?;
        let entity_ref = self.world.entity(entity);
        let display_name = entity_ref
            .get::<DisplayName>()
            .map(|d| d.0.clone())
            .unwrap_or_else(|| format!("{:?}", kind));
        let economy = entity_ref
            .get::<EconomyProfile>()
            .copied()
            .unwrap_or_default();
        let storage = entity_ref
            .get::<StorageProfile>()
            .copied()
            .unwrap_or_default();
        let build_power = entity_ref.get::<BuildPower>().copied().unwrap_or_default();

        if as_builder {
            Some(UnitEcoStats {
                build_power: build_power.0,
                maintenance_consumption_per_second_energy: economy
                    .maintenance_consumption_per_second_energy,
                unit_id: Some(display_name),
                ..Default::default()
            })
        } else {
            let cost = entity_ref.get::<UnitCostComp>().copied()?;
            Some(UnitEcoStats {
                build_power: 0.0,
                mass_cost: cost.0.mass,
                energy_cost: cost.0.energy,
                build_time: cost.0.build_time,
                production_per_second_mass: economy.production_per_second_mass,
                production_per_second_energy: economy.production_per_second_energy,
                maintenance_consumption_per_second_energy: economy
                    .maintenance_consumption_per_second_energy,
                mass_storage: storage.mass,
                energy_storage: storage.energy,
                unit_id: Some(display_name),
            })
        }
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

    fn load_library() -> BlueprintLibrary {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        BlueprintLibrary::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn units_answers_build_and_upgrade_questions() {
        let units = load_library();

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
        let units = load_library();

        let buildable = units.buildable_by(&UnitKind::Commander);
        assert!(buildable.contains(&UnitKind::Mex(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(!buildable.contains(&UnitKind::Engineer(TechLevel::T1)));
    }

    #[test]
    fn factory_builds_its_tier_engineers() {
        let units = load_library();

        let t1 = units.buildable_by(&UnitKind::Factory(TechLevel::T1));
        assert!(t1.contains(&UnitKind::Engineer(TechLevel::T1)));
        assert!(!t1.contains(&UnitKind::Engineer(TechLevel::T2)));

        let t2 = units.buildable_by(&UnitKind::Factory(TechLevel::T2));
        assert!(t2.contains(&UnitKind::Engineer(TechLevel::T2)));
    }

    #[test]
    fn categories_group_common_units() {
        use super::super::types::{category_of, UnitCategory};

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
        let units = load_library();

        assert!(units.mass_storage(&UnitKind::CapT2Mex) > 0.0);
        assert!(
            (units.production_per_second_mass(&UnitKind::CapT2Mex)
                - units.production_per_second_mass(&UnitKind::Mex(TechLevel::T2)))
            .abs()
                < 1e-9
        );

        assert!(units.mass_storage(&UnitKind::CapT3Mex) > 0.0);
        assert!(
            (units.production_per_second_mass(&UnitKind::CapT3Mex)
                - units.production_per_second_mass(&UnitKind::Mex(TechLevel::T3)))
            .abs()
                < 1e-9
        );

        assert!(units.energy_storage(&UnitKind::EnergyStorage) > 0.0);

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
