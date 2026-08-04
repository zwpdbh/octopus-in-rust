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

        Ok(blueprint)
    }

    fn get_unit_from_search(&self, search: &str) -> Result<Unit> {
        let unit = self
            .index
            .search(search)
            .find(|x| x.id == search || x.description.to_lowercase().contains(search))
            .ok_or(Error::UnitNotFound(search.to_string()))?;

        Ok(unit.clone())
    }

    pub fn get_unit_blueprint_from_search(&self, search: &str) -> Result<UnitBlueprint> {
        let unit = self.get_unit_from_search(search)?;

        todo!()
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

    pub fn get_unit_tech_level(&self, search: &str) -> Result<TechLevel> {
        let unit = self.get_unit_from_search(search)?;
        let tech_level_str = unit
            .tech_level()
            .ok_or(Error::UnitMustHasTechLevel(search.to_string()))?;
        let tech_level = TechLevel::new(tech_level_str)?;

        Ok(tech_level)
    }

    pub fn get_unit_eco_effect(&self, search: &str) -> Result<UnitEffectEcoMetrics> {
        let unit = self.get_unit_from_search(search)?;

        let unit_eco = unit
            .economy
            .clone()
            .ok_or(Error::UnitShouldHaveEconomy(unit))?;

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
