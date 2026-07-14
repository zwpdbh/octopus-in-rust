//! CLI for the FAF build-queue simulator.
//!
//! This client uses `faf-sim-service` to run a simulation locally and emits the
//! streamed events as NDJSON. The CLI only needs to know how to start a
//! simulation in active or passive mode and subscribe to it by id.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use faf_sim::quantities::{StepTime, Time};
use faf_sim::sim::{BuildQueue, SimulationEvent};
use faf_sim::snapshot::{
    energy_available, energy_efficiency, energy_net, mass_net, mass_scaling_active,
    scaled_mass_income,
};
use faf_sim_service::{SimServiceEvent, SimulationId, SimulationReceiver, SimulationService};

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
    /// Output format for Ticked events.
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Raw)]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Emit raw simulation events (the primitive data source).
    Raw,
    /// Emit Ticked events grouped into rates/storage/totals/derived.
    Grouped,
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

/// Run in active mode.
///
/// The service does not auto-step in active mode, so we drive it from a
/// background thread that repeatedly sends `Advance` commands until the
/// simulation reports it is finished.
fn run_active(queue: PathBuf, shared: BuildShared) {
    let (queue, dt, max_time) = parse_queue_and_dt(queue, shared);

    let service = SimulationService::new();
    let id = service.start_active_sim(queue, dt, max_time);
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
fn run_passive(queue: PathBuf, shared: BuildShared, tick_interval_ms: u64) {
    let (queue, dt, max_time) = parse_queue_and_dt(queue, shared);

    let service = SimulationService::new();
    let id = service.start_passive_sim(queue, dt, max_time, tick_interval_ms);
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
