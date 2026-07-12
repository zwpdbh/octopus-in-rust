use serde::{Deserialize, Serialize};

/// General unit identity.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct General {
    pub faction_name: Option<String>,
    pub icon: Option<String>,
    pub unit_name: Option<String>,
}

/// Shield-specific stats nested inside [`Defense`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Shield {
    pub shield_max_health: Option<f64>,
    pub shield_regen_rate: Option<f64>,
    pub shield_regen_start_time: Option<f64>,
    pub shield_recharge_time: Option<f64>,
    pub shield_size: Option<f64>,
    pub shield_spill_over_damage_mod: Option<f64>,
    pub personal_shield: Option<bool>,
    pub personal_bubble: Option<bool>,
}

/// Defense and shield stats.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Defense {
    pub health: Option<f64>,
    pub regen_rate: Option<f64>,
    #[serde(default)]
    pub shield: Option<Shield>,
}

/// Static stats describing what it costs to build this unit.
///
/// This is a semantic view over the raw blueprint fields: build cost and build
/// time describe the *target* being constructed, independent of who builds it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BuildTargetStats {
    pub build_cost_mass: f64,
    pub build_cost_energy: f64,
    pub build_time: f64,
}

/// Capability of this unit to build other units.
///
/// `build_rate` is a property of the *builder*, not the target. Keeping it
/// separate from [`BuildTargetStats`] makes that distinction explicit.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct BuilderCapability {
    pub build_rate: f64,
}

/// Economy: production, storage, and raw blueprint fields.
///
/// For semantic access, use [`Economy::target_stats`] and
/// [`Economy::builder_capability`].
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Economy {
    pub build_cost_mass: Option<f64>,
    pub build_cost_energy: Option<f64>,
    pub build_time: Option<f64>,
    pub build_rate: Option<f64>,
    pub production_per_second_mass: Option<f64>,
    pub production_per_second_energy: Option<f64>,
    pub maintenance_consumption_per_second_energy: Option<f64>,
    pub storage_mass: Option<f64>,
    pub storage_energy: Option<f64>,
}

impl Economy {
    /// Extract the target stats if all required fields are present.
    pub fn target_stats(&self) -> Option<BuildTargetStats> {
        Some(BuildTargetStats {
            build_cost_mass: self.build_cost_mass?,
            build_cost_energy: self.build_cost_energy?,
            build_time: self.build_time?,
        })
    }

    /// Extract the builder capability if the build rate is present.
    pub fn builder_capability(&self) -> Option<BuilderCapability> {
        Some(BuilderCapability {
            build_rate: self.build_rate?,
        })
    }
}

/// Vision, radar, sonar and stealth radii.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Intel {
    pub vision_radius: Option<f64>,
    pub water_vision_radius: Option<f64>,
    pub radar_radius: Option<f64>,
    pub sonar_radius: Option<f64>,
    pub radar_stealth_field_radius: Option<f64>,
    pub sonar_stealth_field_radius: Option<f64>,
    pub reactivate_time: Option<f64>,
    pub max_vision_radius: Option<f64>,
    pub min_vision_radius: Option<f64>,
}

/// Movement and physics properties.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Physics {
    pub max_speed: Option<f64>,
    pub turn_rate: Option<f64>,
    pub back_up_distance: Option<f64>,
    pub elevation: Option<f64>,
    pub fuel_use_time: Option<f64>,
    pub fuel_recharge_rate: Option<f64>,
    pub sniper_mode_speed_multiplier: Option<f64>,
    pub water_speed_multiplier: Option<f64>,
    pub land_speed_multiplier: Option<f64>,
    pub sub_speed_multiplier: Option<f64>,
}

/// Air movement stats.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Air {
    pub max_airspeed: Option<f64>,
    pub min_airspeed: Option<f64>,
    pub turn_speed: Option<f64>,
    pub combat_turn_speed: Option<f64>,
    pub start_turn_distance: Option<f64>,
}

/// Display information.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Display {
    #[serde(default)]
    pub abilities: Vec<String>,
}

/// Transport capacity.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Transport {
    pub air_class: Option<bool>,
    pub transport_class: Option<i64>,
    pub slots_small: Option<i64>,
    pub slots_medium: Option<i64>,
    pub slots_large: Option<i64>,
    pub class1_capacity: Option<i64>,
    pub class2_attach_size: Option<i64>,
    pub class3_attach_size: Option<i64>,
    pub can_fire_from_transport: Option<bool>,
}

/// Wreckage reclaim values.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Wreckage {
    pub mass_mult: Option<f64>,
    pub health_mult: Option<f64>,
}

/// Commander / SCU upgrade.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Enhancement {
    pub name: Option<String>,
    pub slot: Option<String>,
    #[serde(default)]
    pub remove_enhancements: Vec<String>,
    pub prerequisite: Option<String>,

    pub build_cost_mass: Option<f64>,
    pub build_cost_energy: Option<f64>,
    pub build_time: Option<f64>,

    pub maintenance_consumption_per_second_energy: Option<f64>,

    pub new_max_radius: Option<f64>,
    pub new_damage_radius_mod: Option<f64>,
    pub new_damage_radius: Option<f64>,
    pub new_rate_of_fire: Option<f64>,

    pub new_health: Option<f64>,
    pub new_regen_rate: Option<f64>,
    pub new_omni_radius: Option<f64>,
    pub new_build_rate: Option<f64>,

    pub additional_damage: Option<f64>,
    pub production_per_second_mass: Option<f64>,
    pub production_per_second_energy: Option<f64>,

    pub personal_shield: Option<bool>,
    pub shield_max_health: Option<f64>,
    pub shield_regen_rate: Option<f64>,
    pub shield_size: Option<f64>,
    pub shield_recharge_time: Option<f64>,
    pub shield_regen_start_time: Option<f64>,

    pub radius: Option<f64>,
    pub regen_ceiling_scu: Option<f64>,
    pub regen_ceiling_t1: Option<f64>,
    pub regen_ceiling_t2: Option<f64>,
    pub regen_ceiling_t3: Option<f64>,
    pub regen_ceiling_t4: Option<f64>,
    pub regen_floor: Option<f64>,
    pub regen_per_second: Option<f64>,
    pub max_health_factor: Option<f64>,
}
