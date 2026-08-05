use faf_blueprints::ConstructionPlan;

/// A simulation is a interface to the faf-game-engine.
/// User could use method on Simulation to control the behavior of a faf-game-engine app.
pub struct Simulation {
    construction_plan: ConstructionPlan,
}

impl Simulation {
    pub fn new(construction_plan: ConstructionPlan) -> Simulation {
        Self { construction_plan }
    }
}
