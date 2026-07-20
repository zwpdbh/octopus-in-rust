//! The `build` command: run a FAF build-queue simulation and emit events.
//!
//! Most runs go through `faf-sim-service` so they can stream live events and
//! support a post-queue tail. When `--tail-seconds 0` is used, the CLI falls
//! back to a direct `Simulation` loop — the same fast path the dataset
//! generator uses.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use faf_quantities::{StepTime, Time};
use faf_sim::sim::{Simulation, SimulationEvent};
use faf_sim::snapshot::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};
use faf_sim_service::{SimServiceEvent, SimulationId, SimulationReceiver, SimulationService};
use faf_sim_shared::BuildQueue;

use crate::command_line::{BuildMode, BuildShared, OutputFormat};

/// Entry point for the `build` command.
pub fn run(mode: BuildMode) {
    match mode {
        BuildMode::Active { queue, shared } => {
            let (queue, dt, max_time) = parse_queue_and_dt(queue, shared);
            if shared.tail_seconds == 0.0 {
                run_direct(queue, dt, max_time, shared.format);
            } else {
                run_active(queue, dt, max_time, shared);
            }
        }
        BuildMode::Passive {
            queue,
            shared,
            tick_interval_ms,
        } => {
            let (queue, dt, max_time) = parse_queue_and_dt(queue, shared);
            if shared.tail_seconds == 0.0 {
                run_direct(queue, dt, max_time, shared.format);
            } else {
                run_passive(queue, dt, max_time, shared, tick_interval_ms);
            }
        }
    }
}

/// Parse the build queue and validate the step time from CLI arguments.
fn parse_queue_and_dt(queue: PathBuf, shared: BuildShared) -> (BuildQueue, StepTime, Option<Time>) {
    let json = std::fs::read_to_string(&queue).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", queue.display(), e);
        std::process::exit(1);
    });
    let queue: BuildQueue = serde_json::from_str(&json).unwrap_or_else(|e| {
        eprintln!("Failed to parse build queue: {}", e);
        std::process::exit(1);
    });

    let dt = StepTime::from_seconds(shared.dt_seconds).unwrap_or_else(|| {
        eprintln!("dt_seconds must be >= 1");
        std::process::exit(1);
    });
    let max_time = shared.max_time_seconds.map(|s| Time::from_raw(s as f64));

    (queue, dt, max_time)
}

/// Subscribe to a simulation, exiting the process on failure.
fn subscribe(service: &SimulationService, id: SimulationId) -> SimulationReceiver {
    service.subscribe(id).unwrap_or_else(|e| {
        eprintln!("Failed to subscribe to simulation: {e}");
        std::process::exit(1);
    })
}

/// Run a simulation directly in the CLI process and print only the final tick.
///
/// This is the same path the dataset generator uses: no service, no channels,
/// no real-time delays, and no per-tick stdout output. It is used whenever
/// `--tail-seconds 0` is passed so final-only runs are as fast as possible.
fn run_direct(queue: BuildQueue, dt: StepTime, max_time: Option<Time>, format: OutputFormat) {
    let mut sim = Simulation::new(queue, dt, max_time, None);
    let mut last_tick: Option<faf_sim::sim::EcoSnapshot> = None;

    while !sim.is_finished() {
        for event in sim.step() {
            if let SimulationEvent::Ticked(snapshot) = event {
                last_tick = Some(*snapshot);
            }
        }
    }

    if let Some(snapshot) = last_tick {
        print_event(&SimulationEvent::Ticked(snapshot), format);
    }
}

/// Run in active mode.
///
/// The service does not auto-step in active mode, so we drive it from a
/// background thread that repeatedly sends `Advance` commands until the
/// simulation reports it is finished.
fn run_active(queue: BuildQueue, dt: StepTime, max_time: Option<Time>, shared: BuildShared) {
    let tail_seconds = Some(shared.tail_seconds);
    let service = SimulationService::new();
    let id = service.start_active_sim_with_tail(queue, dt, max_time, tail_seconds);
    let rx = subscribe(&service, id);

    let is_finished = Arc::new(AtomicBool::new(false));
    let driver = service.clone();
    let finished = Arc::clone(&is_finished);
    std::thread::spawn(move || {
        while !finished.load(Ordering::SeqCst) {
            if driver.advance(id, dt).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    consume_events(rx, &is_finished, shared.format);
}

/// Run in passive mode.
///
/// The service auto-steps, so we only need to consume and print events.
fn run_passive(
    queue: BuildQueue,
    dt: StepTime,
    max_time: Option<Time>,
    shared: BuildShared,
    tick_interval_ms: u64,
) {
    let tail_seconds = Some(shared.tail_seconds);
    let service = SimulationService::new();
    let id =
        service.start_passive_sim_with_tail(queue, dt, max_time, tail_seconds, tick_interval_ms);
    let rx = subscribe(&service, id);

    let is_finished = Arc::new(AtomicBool::new(false));
    consume_events(rx, &is_finished, shared.format);
}

/// Print simulation events as NDJSON until the stream ends or the simulation
/// finishes.
fn consume_events(rx: SimulationReceiver, is_finished: &Arc<AtomicBool>, format: OutputFormat) {
    while let Ok(event) = rx.recv() {
        match event {
            SimServiceEvent::Simulation(sim_event) => {
                print_event(&sim_event, format);

                if matches!(sim_event, SimulationEvent::Finished) {
                    is_finished.store(true, Ordering::SeqCst);
                    break;
                }
            }
            SimServiceEvent::Control(_) => {
                // Control events are not emitted as NDJSON by the CLI.
            }
        }
    }
}

fn print_event(event: &SimulationEvent, format: OutputFormat) {
    match event {
        SimulationEvent::Ticked(snapshot) if format == OutputFormat::Grouped => {
            let grouped = grouped_tick_json(snapshot);
            println!(
                "{}",
                serde_json::to_string(&grouped).expect("serialize grouped tick")
            );
        }
        _ => {
            println!("{}", serde_json::to_string(event).expect("serialize event"));
        }
    }
}

fn grouped_tick_json(s: &faf_sim::sim::EcoSnapshot) -> serde_json::Value {
    serde_json::json!({
        "Ticked": {
            "time": s.time,
            "rates": {
                "production_per_second_mass": s.production_per_second_mass,
                "production_per_second_energy": s.production_per_second_energy,
                "maintenance_consumption_per_second_energy": s.maintenance_consumption_per_second_energy,
                "mass_drain": s.mass_drain,
                "energy_drain": s.energy_drain,
            },
            "storage": {
                "mass_storage": s.mass_storage,
                "mass_storage_cap": s.mass_storage_cap,
                "energy_storage": s.energy_storage,
                "energy_storage_cap": s.energy_storage_cap,
            },
            "totals": {
                "total_mass_spent": s.total_mass_spent,
                "total_energy_spent": s.total_energy_spent,
            },
            "derived": {
                "energy_available": energy_available(s),
                "energy_net": energy_net(s),
                "scaled_mass_income": scaled_mass_income(s),
                "mass_net": mass_net(s),
                "energy_efficiency": energy_efficiency(s),
                "mass_scaling_active": mass_scaling_active(s),
            }
        }
    })
}
