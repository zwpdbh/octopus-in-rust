use anyhow::{bail, Result};

/// Top-level application selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    FafDb,
    FafSim,
    Qqbot,
}

/// Commands that apply to the whole workspace rather than a single app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalCommand {
    Test,
}

/// Parsed command-line invocation.
#[derive(Debug, Clone)]
pub enum Task {
    App {
        app: App,
        command: String,
        rest: Vec<String>,
    },
    Global(GlobalCommand),
}

impl Task {
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let first = args.next().unwrap_or_else(|| "help".to_string());

        match first.as_str() {
            "faf" => Self::parse_app(App::FafDb, args, "help"),
            "faf-sim" => Self::parse_app(App::FafSim, args, "run"),
            "qqbot" => Self::parse_app(App::Qqbot, args, "help"),
            "test" => Ok(Task::Global(GlobalCommand::Test)),
            "help" | "-h" | "--help" => {
                print_top_help();
                std::process::exit(0);
            }
            other => {
                bail!("unknown command '{}'. Run `cargo xtask` for help.", other);
            }
        }
    }

    fn parse_app(
        app: App,
        mut args: impl Iterator<Item = String>,
        default_command: &str,
    ) -> Result<Task> {
        let command = args.next().unwrap_or_else(|| default_command.to_string());
        let rest: Vec<String> = args.collect();
        Ok(Task::App { app, command, rest })
    }
}

pub fn print_faf_db_help() {
    println!("cargo xtask faf — run the FAF unit database");
    println!();
    println!("Usage:");
    println!("  cargo xtask faf <command>");
    println!();
    println!("Commands:");
    println!("  backend    Start the Axum backend (cargo run --package faf-db-server)");
    println!("             Logs are written to data/logs/faf-db-server.log");
    println!("  frontend   Start the Dioxus dev server (dx serve --platform web)");
    println!();
    println!("Examples:");
    println!("  cargo xtask faf backend");
    println!("  cargo xtask faf frontend");
}

pub fn print_faf_sim_help() {
    println!("cargo xtask faf-sim — run and serve the FAF eco/build simulator");
    println!();
    println!("Usage:");
    println!("  cargo xtask faf-sim [command] [options]");
    println!();
    println!("Commands:");
    println!("  run        Run the native simulator (default)");
    println!("  web        Build and serve the WASM bundle");
    println!();
    println!("Native options:");
    println!("  --release  Use the release profile");
    println!();
    println!("Web options:");
    println!("  build      Build the WASM binary and run wasm-bindgen");
    println!("  serve      Build the bundle and start the embedded Axum server (default)");
    println!("  --release  Use the release profile for all builds");
    println!("  --port N   Port for the Axum server (default: 8080)");
    println!();
    println!("Examples:");
    println!("  cargo xtask faf-sim");
    println!("  cargo xtask faf-sim --release");
    println!("  cargo xtask faf-sim web build --release");
    println!("  cargo xtask faf-sim web serve --port 3000");
}

pub fn print_top_help() {
    println!("xtask — development tasks for the Octopus workspace");
    println!();
    println!("Usage: cargo xtask <app> <command> [args]");
    println!("       cargo xtask <global-command>");
    println!();
    println!("Apps:");
    println!("  faf          FAF unit database (Dioxus frontend + Axum backend)");
    println!("  faf-sim      FAF eco/build simulator");
    println!("  qqbot        QQ bot service manager");
    println!();
    println!("Global commands:");
    println!("  test         Run cargo test --workspace");
    println!();
    println!("Examples:");
    println!("  cargo xtask faf backend");
    println!("  cargo xtask faf frontend");
    println!("  cargo xtask faf-sim");
    println!("  cargo xtask faf-sim --release");
    println!("  cargo xtask faf-sim web");
    println!("  cargo xtask faf-sim web build --release");
    println!("  cargo xtask faf-sim web serve --port 3000");
    println!("  cargo xtask qqbot build");
    println!("  cargo xtask qqbot start");
    println!("  cargo xtask qqbot status");
    println!("  cargo xtask qqbot logs core -n 50");
    println!("  cargo xtask qqbot deploy");
    println!("  cargo xtask qqbot remote-status");
    println!("  cargo xtask test");
}
