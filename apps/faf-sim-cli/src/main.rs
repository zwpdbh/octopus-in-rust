//! CLI for the FAF build-queue simulator.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "faf-sim", about = "Headless FAF build-queue simulator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a build-queue simulation and emit events as NDJSON.
    Simulate {
        /// JSON file describing the build queue.
        queue: PathBuf,
        /// Simulation step size in seconds.
        #[arg(short, long, default_value = "0.1")]
        dt: f64,
        /// Maximum simulation time in seconds.
        #[arg(short, long, default_value = "3600.0")]
        max_time: f64,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Simulate {
            queue,
            dt,
            max_time,
        } => run_simulate(queue, dt, max_time),
    }
}

fn run_simulate(queue: PathBuf, dt: f64, max_time: f64) {
    let json = std::fs::read_to_string(&queue).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", queue.display(), e);
        std::process::exit(1);
    });
    let queue: faf_sim::sim::BuildQueue = serde_json::from_str(&json).unwrap_or_else(|e| {
        eprintln!("Failed to parse build queue: {}", e);
        std::process::exit(1);
    });

    let mut sim = faf_sim::sim::Simulation::new(queue, dt, max_time);
    while !sim.is_finished() {
        for event in sim.step() {
            println!("{}", serde_json::to_string(event).expect("serialize event"));
        }
    }
}
