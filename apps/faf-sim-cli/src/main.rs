//! CLI for the FAF build-queue simulator.
//!
//! This client uses `faf-sim-service` directly to run a simulation locally and
//! emits the streamed events as NDJSON.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use faf_sim::protocol::SimulationMode;
use faf_sim::quantities::{StepTime, Time};
use faf_sim::sim::{BuildQueue, SimulationEvent};
use faf_sim_service::{
    RunConfig, SimServiceEvent, SimulationId, SimulationReceiver, SimulationService,
};

#[derive(Parser, Debug)]
#[command(name = "faf-sim", about = "Headless FAF build-queue simulator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a build-queue simulation and emit events as NDJSON.
    Build {
        #[command(subcommand)]
        mode: BuildMode,
    },
}

#[derive(Subcommand, Debug)]
enum BuildMode {
    /// Run the simulation in active (manual-advance) mode.
    Active {
        /// JSON file describing the build queue.
        queue: PathBuf,
        #[command(flatten)]
        shared: BuildShared,
    },
    /// Run the simulation in passive (auto-play) mode.
    Passive {
        /// JSON file describing the build queue.
        queue: PathBuf,
        #[command(flatten)]
        shared: BuildShared,
        /// Real-world delay between simulation steps in milliseconds.
        #[arg(long, default_value = "50")]
        tick_interval_ms: u64,
    },
}

#[derive(Args, Debug, Clone, Copy)]
struct BuildShared {
    /// Simulation step size in seconds. Must be an integer >= 1.
    #[arg(short, long, default_value = "1")]
    dt_seconds: u32,
    /// Maximum simulation time in seconds. When omitted the simulation
    /// runs until the build queue is empty.
    #[arg(short, long)]
    max_time_seconds: Option<u32>,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { mode } => match mode {
            BuildMode::Active { queue, shared } => run_active(queue, shared),
            BuildMode::Passive {
                queue,
                shared,
                tick_interval_ms,
            } => run_passive(queue, shared, tick_interval_ms),
        },
    }
}

/// Shared setup: parse the queue, validate step time, start the simulation, and
/// subscribe to its event stream.
fn prepare_simulation(
    queue: PathBuf,
    shared: BuildShared,
    mode: SimulationMode,
) -> (
    SimulationService,
    SimulationId,
    SimulationReceiver,
    StepTime,
) {
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

    let service = SimulationService::new();
    let config = RunConfig { dt, max_time, mode };

    let id = service.start(queue, config);
    let rx = service.subscribe(id).unwrap_or_else(|e| {
        eprintln!("Failed to subscribe to simulation: {e}");
        std::process::exit(1);
    });

    (service, id, rx, dt)
}

/// Run in active mode.
///
/// The service does not auto-step in active mode, so we drive it from a
/// background thread that repeatedly sends `Advance` commands until the
/// simulation reports it is finished.
fn run_active(queue: PathBuf, shared: BuildShared) {
    let (service, id, rx, dt) = prepare_simulation(queue, shared, SimulationMode::Active);

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

    consume_events(rx, &is_finished);
}

/// Run in passive mode.
///
/// The service auto-steps, so we only need to consume and print events.
fn run_passive(queue: PathBuf, shared: BuildShared, tick_interval_ms: u64) {
    let (_service, _id, rx, _dt) =
        prepare_simulation(queue, shared, SimulationMode::Passive { tick_interval_ms });
    let is_finished = Arc::new(AtomicBool::new(false));
    consume_events(rx, &is_finished);
}

/// Print simulation events as NDJSON until the stream ends or the simulation
/// finishes.
fn consume_events(rx: SimulationReceiver, is_finished: &Arc<AtomicBool>) {
    while let Ok(event) = rx.recv() {
        match event {
            SimServiceEvent::Simulation(sim_event) => {
                println!(
                    "{}",
                    serde_json::to_string(&sim_event).expect("serialize event")
                );
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
