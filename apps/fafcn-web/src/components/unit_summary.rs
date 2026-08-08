use faf_blueprints::{TechLevel, UnitBlueprint, UnitCostMetrics, UnitEffectEcoMetrics};
use serde::Deserialize;

use crate::utils::faction_from_id;

/// Lightweight unit summary sent by `/api/units` and used throughout the UI.
#[derive(Clone, Deserialize, PartialEq)]
pub struct UnitSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub faction: String,
    pub tech_level: TechLevel,
    pub cost: UnitCostMetrics,
    pub eco_effect: UnitEffectEcoMetrics,
    pub category: Option<String>,
    pub kind: Option<String>,
    pub strategic_icon_name: Option<String>,
}

impl UnitSummary {
    /// Build a full blueprint from the summary fields.
    pub fn to_blueprint(&self) -> UnitBlueprint {
        UnitBlueprint::new(
            self.id.clone(),
            self.name.clone(),
            self.cost,
            self.eco_effect.clone(),
            self.tech_level,
            None,
            None,
            self.strategic_icon_name.clone(),
        )
    }

    /// Reconstruct a summary from a blueprint so queue cards can display it.
    pub fn from_blueprint(bp: &UnitBlueprint) -> Self {
        Self {
            id: bp.unit_id().to_string(),
            name: bp.unit_description().to_string(),
            description: bp.unit_description().to_string(),
            faction: faction_from_id(bp.unit_id()).to_string(),
            tech_level: bp.tech_level(),
            cost: bp.unit_cost(),
            eco_effect: bp.unit_eco_effect().clone(),
            category: bp.category().map(|c| c.label().to_string()),
            kind: bp.kind().map(|k| k.label().to_string()),
            strategic_icon_name: bp.strategic_icon_name().map(|s| s.to_string()),
        }
    }
}
