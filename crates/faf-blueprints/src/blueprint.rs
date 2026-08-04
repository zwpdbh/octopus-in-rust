#![allow(unused)]

use crate::{categories::*, eco_metrics::*};
use crate::{Error, Result};
use faf_units::{FafUnitIndex, Unit};
/// Unified repository of unit knowledge backed by a Bevy ECS blueprint world.
///
/// `BlueprintLibrary` is self-contained: after construction it no longer
/// references the raw `DataIndex`. All build/upgrade rules are explicit recipes
/// rather than derived string-category graphs.
#[derive(Debug)]
pub struct FafBlueprints {
    index: FafUnitIndex,
}

#[derive(Debug, Clone)]
pub struct UnitBlueprint {
    unit_id: String,
    unit_description: String,
    unit_cost: UnitCostEcoMetrics,
    unit_eco_effect: UnitEffectEcoMetrics,
    tech_level: TechLevel,
}

impl FafBlueprints {
    pub fn new() -> Result<Self> {
        let blueprint = FafBlueprints {
            index: FafUnitIndex::default()?,
        };
        println!("loaded {} units", blueprint.index.units.len());
        Ok(blueprint)
    }

    pub fn get_units_from_search(&self, search: &str) -> Result<Vec<UnitBlueprint>> {
        let units = self.get_unit_from_search(search)?;
        let mut blueprints: Vec<UnitBlueprint> = Vec::new();

        for each in units {
            let eco_metrics = self.get_eco_cost_from_search(&each)?;
            let eco_effect = self.get_unit_eco_effect(&each)?;
            let tech_level = self.get_unit_tech_level(&each)?;

            let unit_blueprint = UnitBlueprint {
                unit_id: each.id,
                unit_description: each.description,
                unit_cost: eco_metrics,
                unit_eco_effect: eco_effect,
                tech_level: tech_level,
            };
            blueprints.push(unit_blueprint);
        }
        Ok(blueprints)
    }

    fn get_unit_from_search(&self, search: &str) -> Result<Vec<Unit>> {
        let units: Vec<Unit> = self.index.search(&search).map(|u| u.clone()).collect();

        Ok(units)
    }

    fn get_eco_cost_from_search(&self, unit: &Unit) -> Result<UnitCostEcoMetrics> {
        let unit_eco = unit
            .economy
            .clone()
            .ok_or(Error::UnitShouldHaveEconomy(unit.clone()))?;

        let build_cost_mass = unit_eco.build_cost_mass.unwrap_or(0.0);
        let build_cost_energy = unit_eco.build_cost_energy.unwrap_or(0.0);
        let build_time = unit_eco.build_time.unwrap_or(0.0);
        let eco_metrics = UnitCostEcoMetrics::new(build_cost_mass, build_cost_energy, build_time);
        Ok(eco_metrics)
    }

    fn get_unit_tech_level(&self, unit: &Unit) -> Result<TechLevel> {
        let tech_level_str = unit
            .tech_level()
            .ok_or(Error::UnitMustHasTechLevel(unit.clone()))?;
        let tech_level = TechLevel::new(tech_level_str)?;

        Ok(tech_level)
    }

    fn get_unit_eco_effect(&self, unit: &Unit) -> Result<UnitEffectEcoMetrics> {
        let unit_eco = unit
            .economy
            .clone()
            .ok_or(Error::UnitShouldHaveEconomy(unit.clone()))?;

        let generate_mass_rate = unit_eco.production_per_second_mass.unwrap_or(0.0);
        let generate_energy_rate = unit_eco.production_per_second_energy.unwrap_or(0.0);
        let maintainance_energy_drain = unit_eco
            .maintenance_consumption_per_second_energy
            .unwrap_or(0.0);
        let increase_mass_storage_capacity = unit_eco.storage_mass.unwrap_or(0.0);
        let increase_energy_storage_capacity = unit_eco.storage_energy.unwrap_or(0.0);

        Ok(UnitEffectEcoMetrics::new(
            generate_mass_rate,
            generate_energy_rate,
            maintainance_energy_drain,
            increase_mass_storage_capacity,
            increase_energy_storage_capacity,
        ))
    }
}
