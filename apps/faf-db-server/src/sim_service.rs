//! Synchronous simulation runner that wraps `faf_sim::Simulation`.
//!
//! The `SimulationService` in this crate owns a dedicated OS thread for the
//! Bevy `App` (which is not `Send`) and exposes events through a
//! `crossbeam_channel` receiver. Callers can bridge this synchronous stream
//! into their own async runtime.

use crossbeam_channel::{unbounded, Receiver};
use faf_sim::sim::{BuildQueue, Simulation, SimulationEvent};

/// Start a simulation on a background thread and return a receiver for its
/// event stream.
pub fn run(queue: BuildQueue, dt: f64, max_time: Option<f64>) -> Receiver<SimulationEvent> {
    let (tx, rx) = unbounded::<SimulationEvent>();

    std::thread::spawn(move || {
        let mut sim = Simulation::new(queue, dt, max_time);
        while !sim.is_finished() {
            for event in sim.step() {
                let event = event.clone();
                let is_finished = matches!(event, SimulationEvent::Finished);
                if tx.send(event).is_err() || is_finished {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    rx
}
