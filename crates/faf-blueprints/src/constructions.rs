use crate::{eco_metrics::*, UnitBlueprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConstructionPlan {
    player_eco: PlayerEcoMetrics,
    building_queue: Vec<ConstructionAction>,
}

impl ConstructionPlan {
    pub fn player_eco(&self) -> &PlayerEcoMetrics {
        &self.player_eco
    }

    pub fn building_queue(&self) -> &[ConstructionAction] {
        &self.building_queue
    }

    pub fn into_parts(self) -> (PlayerEcoMetrics, Vec<ConstructionAction>) {
        (self.player_eco, self.building_queue)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConstructionAction {
    builders: Vec<UnitBlueprint>,
    target: UnitBlueprint,
}

impl ConstructionAction {
    pub fn builders(&self) -> &[UnitBlueprint] {
        &self.builders
    }

    pub fn target(&self) -> &UnitBlueprint {
        &self.target
    }
}
