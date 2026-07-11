use crate::types::ConstructionPlan;

const PLAN_STORAGE_KEY: &str = "faf-db-construction-plan-v7";

pub fn save_plan_to_storage(plan: &ConstructionPlan) {
    if let Ok(json) = serde_json::to_string(plan) {
        let _ = web_sys::window()
            .and_then(|w| w.local_storage().ok()?)
            .map(|storage| storage.set_item(PLAN_STORAGE_KEY, &json));
    }
}

pub fn load_plan_from_storage() -> Option<ConstructionPlan> {
    let json = web_sys::window()
        .and_then(|w| w.local_storage().ok()?)
        .and_then(|storage| storage.get_item(PLAN_STORAGE_KEY).ok()?)?;
    serde_json::from_str(&json).ok()
}
