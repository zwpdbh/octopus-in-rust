//! CLI for the FAF build-queue simulator.
//!
//! This client connects to the `faf-db-server` WebSocket simulation endpoint,
//! sends a build queue, and prints the streamed events as NDJSON.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use faf_sim::protocol::{SimClientMessage, SimServerMessage};
use faf_sim::sim::{BuildQueue, SimulationEvent};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

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
        /// WebSocket URL of the simulation server.
        #[arg(short, long, default_value = "ws://localhost:8081/ws/simulate")]
        url: String,
        /// Simulation resolution in steps per second.
        #[arg(short, long, default_value = "10")]
        resolution: u32,
        /// Maximum simulation time in seconds. When omitted the simulation
        /// runs until the build queue is empty.
        #[arg(short, long)]
        max_time: Option<f64>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            queue,
            url,
            resolution,
            max_time,
        } => run_simulate(queue, url, resolution, max_time).await,
    }
}

async fn run_simulate(queue: PathBuf, url: String, resolution: u32, max_time: Option<f64>) {
    let json = std::fs::read_to_string(&queue).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", queue.display(), e);
        std::process::exit(1);
    });
    let queue: BuildQueue = serde_json::from_str(&json).unwrap_or_else(|e| {
        eprintln!("Failed to parse build queue: {}", e);
        std::process::exit(1);
    });

    let (mut ws_stream, _) = connect_async(&url).await.unwrap_or_else(|e| {
        eprintln!("Failed to connect to {}: {}", url, e);
        std::process::exit(1);
    });

    let start = SimClientMessage::Start {
        queue,
        resolution,
        max_time,
    };
    let start_text = serde_json::to_string(&start).expect("serialize start message");
    ws_stream
        .send(Message::Text(start_text))
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to send start message: {}", e);
            std::process::exit(1);
        });

    while let Some(msg) = ws_stream.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                std::process::exit(1);
            }
        };

        let text = match msg {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };

        let server_msg: SimServerMessage = match serde_json::from_str(&text) {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Failed to parse server message: {}", e);
                continue;
            }
        };

        match server_msg {
            SimServerMessage::Event(event) => {
                println!(
                    "{}",
                    serde_json::to_string(&event).expect("serialize event")
                );
                if matches!(event, SimulationEvent::Finished) {
                    break;
                }
            }
            SimServerMessage::Error { message } => {
                eprintln!("Simulation error: {}", message);
                std::process::exit(1);
            }
        }
    }
}
