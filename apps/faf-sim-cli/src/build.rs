//! The `build` command: run a FAF build-queue simulation and emit events.
//!
//! Most runs go through `faf-sim-service` so they can stream live events and
//! support a post-queue tail. When `--tail-seconds 0` is used, the CLI falls
//! back to a direct `Simulation` loop — the same fast path the dataset
//! generator uses.

use anyhow::Result;

#[allow(unused)]
pub fn run(mass: usize, energy: usize, build_time: usize, build_power: usize) -> Result<()> {
    Ok(())
}
