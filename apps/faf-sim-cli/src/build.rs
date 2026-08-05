//! The `build` command: run a FAF build-queue simulation and emit events.
//!
//! Most runs go through `faf-sim-service` so they can stream live events and
//! support a post-queue tail. When `--tail-seconds 0` is used, the CLI falls
//! back to a direct `Simulation` loop — the same fast path the dataset
//! generator uses.

use anyhow::Result;
use faf_blueprints::*;

pub fn run(construction_plan: &str) -> Result<()> {
    let construction_plan: ConstructionPlan =
        serde_json::from_str(construction_plan).map_err(|e| {
            anyhow::anyhow!("failed to parse construction plan: {construction_plan}, error: {e}",)
        })?;
    println!("{:?}", construction_plan);
    Ok(())
}
