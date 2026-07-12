//! CLI for the FAF build-queue simulator.
//!
//! This client uses `faf-sim-service` directly to run a simulation locally and
//! emits the streamed events as NDJSON.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use faf_sim::sim::{BuildQueue, SimulationEvent};
use faf_sim_service::{RunConfig, SimServiceEvent, SimulationService};

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
        /// JSON file describing the build queue.
        queue: PathBuf,
        /// Simulation resolution in steps per second.
        #[arg(short, long, default_value = "10")]
        resolution: u32,
        /// Maximum simulation time in seconds. When omitted the simulation
        /// runs until the build queue is empty.
        #[arg(short, long)]
        max_time: Option<f64>,
        /// Real-world delay between simulation steps in milliseconds.
        #[arg(long, default_value = "50")]
        tick_interval_ms: u64,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            queue,
            resolution,
            max_time,
            tick_interval_ms,
        } => run_simulate(queue, resolution, max_time, tick_interval_ms),
    }
}

fn run_simulate(queue: PathBuf, resolution: u32, max_time: Option<f64>, tick_interval_ms: u64) {
    let json = std::fs::read_to_string(&queue).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", queue.display(), e);
        std::process::exit(1);
    });
    let queue: BuildQueue = serde_json::from_str(&json).unwrap_or_else(|e| {
        eprintln!("Failed to parse build queue: {}", e);
        std::process::exit(1);
    });

    let service = SimulationService::new();
    let config = RunConfig {
        dt: 1.0 / resolution as f64,
        max_time,
        tick_interval: Duration::from_millis(tick_interval_ms),
    };
    let (_id, rx) = service.start(queue, config);

    while let Ok(event) = rx.recv() {
        match event {
            SimServiceEvent::Simulation(sim_event) => {
                println!(
                    "{}",
                    serde_json::to_string(&sim_event).expect("serialize event")
                );
                if matches!(sim_event, SimulationEvent::Finished) {
                    break;
                }
            }
            SimServiceEvent::Control(_) => {
                // Control events are not emitted as NDJSON by the CLI.
            }
        }
    }
}
