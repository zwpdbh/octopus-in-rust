use crate::{eco_metrics::*, UnitBlueprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConstructionPlan {
    player_eco: PlayerEcoMetrics,
    building_queue: Vec<ConstructionAction>,
}

impl ConstructionPlan {
    pub fn new(player_eco: PlayerEcoMetrics, building_queue: Vec<ConstructionAction>) -> Self {
        Self {
            player_eco,
            building_queue,
        }
    }

    pub fn player_eco(&self) -> &PlayerEcoMetrics {
        &self.player_eco
    }

    pub fn building_queue(&self) -> &[ConstructionAction] {
        &self.building_queue
    }

    pub fn into_parts(self) -> (PlayerEcoMetrics, Vec<ConstructionAction>) {
        (self.player_eco, self.building_queue)
    }

    pub fn set_player_eco(&mut self, eco: PlayerEcoMetrics) {
        self.player_eco = eco;
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ConstructionAction {
    builders: Vec<UnitBlueprint>,
    target: UnitBlueprint,
}

impl ConstructionAction {
    pub fn new(builders: Vec<UnitBlueprint>, target: UnitBlueprint) -> Self {
        Self { builders, target }
    }

    pub fn builders(&self) -> &[UnitBlueprint] {
        &self.builders
    }

    pub fn target(&self) -> &UnitBlueprint {
        &self.target
    }

    pub fn set_builders(&mut self, builders: Vec<UnitBlueprint>) {
        self.builders = builders;
    }

    pub fn set_target(&mut self, target: UnitBlueprint) {
        self.target = target;
    }
}
