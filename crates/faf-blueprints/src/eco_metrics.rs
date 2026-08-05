use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlayerEcoMetrics {
    // mass produce vs consume
    pub mass_generate_rate: f64,
    pub mass_drain: f64,

    // energy produce vs consume
    pub energy_generate_rate: f64,
    pub energy_drain: f64,

    // storage related
    pub mass_in_storage: f64,
    pub max_capacity_in_mass_storage: f64,
    pub energy_in_storage: f64,
    pub max_capacity_in_energy_storage: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnitCostEcoMetrics {
    pub mass: f64,
    pub energy: f64,
    pub build_time: f64,
}

impl UnitCostEcoMetrics {
    pub fn new(mass: f64, energy: f64, build_time: f64) -> Self {
        UnitCostEcoMetrics {
            mass,
            energy,
            build_time,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnitEffectEcoMetrics {
    pub generate_mass_rate: f64,
    pub generate_energy_rate: f64,
    pub maintainance_energy_drain: f64,
    pub increase_mass_storage_capacity: f64,
    pub increase_energy_storage_capacity: f64,
}

impl UnitEffectEcoMetrics {
    pub fn new(
        generate_mass_rate: f64,
        generate_energy_rate: f64,
        maintainance_energy_drain: f64,
        increase_mass_storage_capacity: f64,
        increase_energy_storage_capacity: f64,
    ) -> Self {
        UnitEffectEcoMetrics {
            generate_mass_rate,
            generate_energy_rate,
            maintainance_energy_drain,
            increase_mass_storage_capacity,
            increase_energy_storage_capacity,
        }
    }
}
