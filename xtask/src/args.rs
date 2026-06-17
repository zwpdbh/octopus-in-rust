use anyhow::{bail, Result};

/// Top-level application selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Qqbot,
}

/// Commands that apply to the whole workspace rather than a single app.
#[derive(Debug, Clone)]
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
            "qqbot" => {
                let command = args.next().unwrap_or_else(|| "help".to_string());
                let rest: Vec<String> = args.collect();
                Ok(Task::App {
                    app: App::Qqbot,
                    command,
                    rest,
                })
            }
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
}

pub fn print_top_help() {
    println!("xtask — development tasks for the Octopus workspace");
    println!();
    println!("Usage: cargo xtask <app> <command> [args]");
    println!();
    println!("Apps:");
    println!("  qqbot        QQ bot service manager");
    println!();
    println!("Global commands:");
    println!("  test         Run cargo test --workspace");
    println!();
    println!("Examples:");
    println!("  cargo xtask qqbot build");
    println!("  cargo xtask qqbot start");
    println!("  cargo xtask qqbot status");
    println!("  cargo xtask qqbot logs core -n 50");
    println!("  cargo xtask test");
}
