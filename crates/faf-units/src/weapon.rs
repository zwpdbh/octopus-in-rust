use serde::{Deserialize, Serialize};

/// A weapon mounted on a unit.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Weapon {
    pub display_name: Option<String>,
    pub label: Option<String>,
    pub weapon_category: Option<String>,
    pub damage_type: Option<String>,

    /// True for weapons that are not real weapons (e.g. teleport).
    #[serde(default)]
    pub dummy_weapon: Option<bool>,

    pub target_restrict_only_allow: Option<String>,
    pub target_restrict_disallow: Option<String>,

    #[serde(default)]
    pub ignore_if_disabled: Option<bool>,

    /// True if the weapon fires when the unit dies.
    #[serde(default)]
    pub fire_on_death: Option<bool>,

    #[serde(default)]
    pub force_single_fire: Option<bool>,

    #[serde(default, deserialize_with = "crate::util::deserialize_bool_or_int")]
    pub manual_fire: Option<bool>,

    pub weapon_unpack_animation: Option<String>,

    pub max_radius: Option<f64>,
    pub min_radius: Option<f64>,

    pub damage: Option<f64>,
    pub damage_radius: Option<f64>,
    pub damage_to_shields: Option<f64>,

    pub initial_damage: Option<f64>,

    pub nuke_inner_ring_radius: Option<f64>,
    pub nuke_outer_ring_radius: Option<f64>,
    pub nuke_inner_ring_damage: Option<f64>,
    pub nuke_outer_ring_damage: Option<f64>,

    pub do_t_time: Option<f64>,
    pub do_t_pulses: Option<f64>,

    pub muzzle_velocity: Option<f64>,

    pub beam_lifetime: Option<f64>,
    pub beam_collision_delay: Option<f64>,

    pub fire_target_layer_caps_table: Option<FireTargetLayerCapsTable>,

    pub firing_randomness: Option<f64>,
    pub firing_randomness_while_moving: Option<f64>,
    pub firing_tolerance: Option<f64>,

    pub turret_yaw_range: Option<f64>,

    pub muzzle_salvo_size: Option<i64>,
    pub muzzle_salvo_delay: Option<f64>,
    pub muzzle_charge_delay: Option<f64>,

    #[serde(default)]
    pub rack_fire_together: Option<bool>,

    pub rack_salvo_size: Option<i64>,
    pub rack_salvo_charge_time: Option<f64>,
    pub rack_salvo_reload_time: Option<f64>,

    pub rate_of_fire: Option<f64>,

    pub tractor_damage: Option<f64>,
    pub tractor_damage_interval: Option<f64>,

    pub projectile_id: Option<String>,

    #[serde(default)]
    pub depth_charge: Option<DepthCharge>,

    pub buffs: Option<serde_json::Value>,

    /// Enriched projectile data (fragmentation, cost, health).
    #[serde(default)]
    pub projectile: Option<Projectile>,

    /// Rack / muzzle bone layout.
    #[serde(default)]
    pub rack_bones: Vec<RackBones>,

    // ------------------------------------------------------------------
    // Enrichment fields added by the generator (not present in raw blueprints).
    // ------------------------------------------------------------------
    /// Number of child projectiles spawned on split/impact.
    #[serde(default)]
    pub child_count: Option<i64>,

    /// How child projectiles split: `onWater`, `onDeath`, etc.
    #[serde(default)]
    pub child_split_type: Option<String>,

    /// True if this weapon is an anti-missile flare.
    #[serde(default)]
    pub is_anti_missile_flare: Option<bool>,

    /// True if this is a torpedo weapon.
    #[serde(default)]
    pub is_torpedo: Option<bool>,

    /// Number of fragments produced on detonation.
    #[serde(default)]
    pub fragment_count: Option<i64>,

    /// Death-stun parameters extracted from unit scripts.
    #[serde(default)]
    pub death_stun_params: Option<serde_json::Value>,
}

/// Which target layers a weapon can fire from and at.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
pub struct FireTargetLayerCapsTable {
    pub air: Option<String>,
    pub land: Option<String>,
    pub seabed: Option<String>,
    pub sub: Option<String>,
    pub water: Option<String>,
}

/// A rack bone entry describes which muzzle bones belong to a rack.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct RackBones {
    #[serde(default)]
    pub muzzle_bones: Vec<serde_json::Value>,
}

/// Depth-charge capability of a torpedo defense weapon.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct DepthCharge {
    pub projectiles_to_deflect: Option<i64>,
    pub radius: Option<f64>,
}

/// Enriched projectile data attached to a weapon.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Projectile {
    pub description: Option<String>,
    pub health: Option<f64>,
    pub build_cost_energy: Option<f64>,
    pub build_cost_mass: Option<f64>,
    pub build_time: Option<f64>,
}

impl Weapon {
    /// The weapon's effective DPS assuming every shot hits.
    ///
    /// This is a rough approximation. Accurate FA DPS requires cycle
    /// simulation and is intentionally left to a dedicated simulator.
    pub fn naive_dps(&self) -> Option<f64> {
        let damage = self.damage?;
        let rate_of_fire = self.rate_of_fire?;
        Some(damage * rate_of_fire)
    }

    /// True if the weapon can target the given layer from any source layer.
    pub fn can_target_layer(&self, target: &str) -> bool {
        let Some(table) = &self.fire_target_layer_caps_table else {
            return false;
        };
        [
            table.air.as_deref(),
            table.land.as_deref(),
            table.seabed.as_deref(),
            table.sub.as_deref(),
            table.water.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|caps| caps.to_lowercase().contains(&target.to_lowercase()))
    }
}
