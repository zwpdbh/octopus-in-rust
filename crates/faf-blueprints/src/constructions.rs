use crate::{eco_metrics::*, UnitBlueprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConstructionPlan {
    player_eco: PlayerEcoMetrics,
    building_queue: Vec<ConstructionAction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConstructionAction {
    builders: Vec<UnitBlueprint>,
    target: UnitBlueprint,
}
