use anyhow::{bail, Context, Result};

/// Top-level application selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Qqbot,
}

/// Commands that apply to the whole workspace rather than a single app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalCommand {
    Test,
}

/// Web-specific workflow commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebCommand {
    Build,
    Serve,
}

/// Parsed command-line invocation.
#[derive(Debug, Clone)]
pub enum Task {
    App {
        app: App,
        command: String,
        rest: Vec<String>,
    },
    Web {
        command: WebCommand,
        release: bool,
        port: u16,
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
            "web" => Self::parse_web(args),
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

    fn parse_web(mut args: impl Iterator<Item = String>) -> Result<Task> {
        let mut command: Option<WebCommand> = None;
        let mut release = false;
        let mut port: Option<u16> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "help" | "-h" | "--help" => {
                    print_web_help();
                    std::process::exit(0);
                }
                "build" => {
                    if command.is_some() {
                        bail!("multiple web commands given");
                    }
                    command = Some(WebCommand::Build);
                }
                "serve" => {
                    if command.is_some() {
                        bail!("multiple web commands given");
                    }
                    command = Some(WebCommand::Serve);
                }
                "--release" => release = true,
                "--port" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--port requires a value"))?;
                    port = Some(
                        value
                            .parse()
                            .with_context(|| format!("invalid port number: {value}"))?,
                    );
                }
                other if other.starts_with("--port=") => {
                    let value = &other["--port=".len()..];
                    port = Some(
                        value
                            .parse()
                            .with_context(|| format!("invalid port number: {value}"))?,
                    );
                }
                other => {
                    bail!("unknown web option '{}'", other);
                }
            }
        }

        Ok(Task::Web {
            command: command.unwrap_or(WebCommand::Serve),
            release,
            port: port.unwrap_or(8080),
        })
    }
}

pub fn print_web_help() {
    println!("cargo xtask web — build and serve the FAF sim WASM bundle");
    println!();
    println!("Usage:");
    println!("  cargo xtask web [command] [options]");
    println!();
    println!("Commands:");
    println!("  build      Build the WASM binary and run wasm-bindgen");
    println!("  serve      Build the bundle and start the embedded Axum server (default)");
    println!();
    println!("Options:");
    println!("  --release  Use the release profile for all builds");
    println!("  --port N   Port for the Axum server (default: 8080)");
    println!();
    println!("Examples:");
    println!("  cargo xtask web");
    println!("  cargo xtask web build --release");
    println!("  cargo xtask web serve --port 3000");
}

pub fn print_top_help() {
    println!("xtask — development tasks for the Octopus workspace");
    println!();
    println!("Usage: cargo xtask <app> <command> [args]");
    println!("       cargo xtask <global-command>");
    println!();
    println!("Apps:");
    println!("  qqbot        QQ bot service manager");
    println!();
    println!("Global commands:");
    println!("  test         Run cargo test --workspace");
    println!();
    println!("Web commands:");
    println!("  web build              Build the Bevy WASM bundle and run wasm-bindgen");
    println!("  web serve              Build the WASM bundle and start the Axum server");
    println!("  web serve --port 3000  Serve on a custom port (default: 8080)");
    println!("  web --release          Use the release profile for builds");
    println!();
    println!("Examples:");
    println!("  cargo xtask qqbot build");
    println!("  cargo xtask qqbot start");
    println!("  cargo xtask qqbot status");
    println!("  cargo xtask qqbot logs core -n 50");
    println!("  cargo xtask qqbot deploy");
    println!("  cargo xtask qqbot remote-status");
    println!("  cargo xtask test");
    println!("  cargo xtask web");
    println!("  cargo xtask web build --release");
}
