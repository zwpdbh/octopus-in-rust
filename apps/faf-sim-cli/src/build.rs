//! The `build` command: run a FAF build-queue simulation and emit events.
//!
//! Most runs go through `faf-sim-service` so they can stream live events and
//! support a post-queue tail. When `--tail-seconds 0` is used, the CLI falls
//! back to a direct `Simulation` loop — the same fast path the dataset
//! generator uses.

use anyhow::Result;
use faf_blueprints::*;
use faf_sim_protocol::SimSpeed;
use faf_sim_service::SimulationService;

/// Run the simulation for the given JSON plan.
///
/// `speed` is interpreted as ticks per wall-clock second.  Values `<= 0`
/// mean "run as fast as possible"; positive values throttle the simulation
/// to that many ticks per second.
pub fn run(construction_plan: &str, speed: f64) -> Result<()> {
    let construction_plan: ConstructionPlan =
        serde_json::from_str(construction_plan).map_err(|e| {
            anyhow::anyhow!("failed to parse construction plan: {construction_plan}, error: {e}",)
        })?;

    let sim_speed = if speed > 0.0 {
        SimSpeed::TicksPerSecond(speed)
    } else {
        // Default for headless runs: finish as fast as the CPU allows.
        SimSpeed::Unlimited
    };

    let service = SimulationService::new();
    service.run_blocking(construction_plan, sim_speed)?;

    println!("simulation finished");
    Ok(())
}
