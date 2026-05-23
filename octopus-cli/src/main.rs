use std::path::PathBuf;
use std::process;

use octopus_cli::app::{OctopusCLI, enable_logging};
use octopus_cli::cli::{AgentChoice, Cli, Commands, InputFormat, OutputFormat};
use octopus_cli::config::load_config_from_string;
use octopus_cli::constant::get_version;
use octopus_cli::session::Session;

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    // Version is handled by clap's version action
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("kimi, version {}", get_version());
        process::exit(0);
    }

    // Handle subcommands
    if let Some(command) = cli.command {
        match command {
            Commands::Login { json } => {
                if json {
                    println!(
                        "{{\"event\":\"error\",\"message\":\"OAuth login is not yet implemented in octopus-cli\"}}"
                    );
                } else {
                    println!("OAuth login is not yet implemented in octopus-cli.");
                }
                return;
            }
            Commands::Logout { json } => {
                if json {
                    println!(
                        "{{\"event\":\"error\",\"message\":\"OAuth logout is not yet implemented in octopus-cli\"}}"
                    );
                } else {
                    println!("OAuth logout is not yet implemented in octopus-cli.");
                }
                return;
            }
            Commands::Term => {
                println!("Term (Toad TUI) is not yet implemented in octopus-cli.");
                return;
            }
            Commands::Acp => {
                println!("ACP server is not yet implemented in octopus-cli.");
                return;
            }
            Commands::BackgroundTaskWorker { .. } => {
                println!("Background task worker is not yet implemented in octopus-cli.");
                return;
            }
            Commands::WebWorker { .. } => {
                println!("Web worker is not yet implemented in octopus-cli.");
                return;
            }
            Commands::Export {
                session_id,
                output,
                yes,
            } => {
                let runtime =
                    tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
                let work_dir = cli
                    .work_dir
                    .map(|p| p.canonicalize().unwrap_or(p))
                    .unwrap_or_else(|| {
                        std::env::current_dir().expect("Failed to get current directory")
                    });
                runtime.block_on(async {
                    if let Err(e) =
                        octopus_cli::cli::export::run_export(&work_dir, session_id, output, yes)
                            .await
                    {
                        eprintln!("Export failed: {}", e);
                        std::process::exit(1);
                    }
                });
                return;
            }
            Commands::Info { json } => {
                octopus_cli::cli::info::run_info(json);
                return;
            }
            Commands::Plugin { command } => {
                match command {
                    Some(octopus_cli::cli::PluginCommands::Install { target }) => {
                        println!("Plugin install not yet implemented: {}", target);
                    }
                    Some(octopus_cli::cli::PluginCommands::List) => {
                        println!("Plugin list not yet implemented");
                    }
                    Some(octopus_cli::cli::PluginCommands::Remove { name }) => {
                        println!("Plugin remove not yet implemented: {}", name);
                    }
                    Some(octopus_cli::cli::PluginCommands::Info { name }) => {
                        println!("Plugin info not yet implemented: {}", name);
                    }
                    None => {
                        println!("Plugin management commands: install, list, remove, info");
                    }
                }
                return;
            }
            Commands::Toad => {
                println!("Toad TUI is not yet implemented in octopus-cli.");
                return;
            }
            Commands::Web => {
                println!("Web UI server is not yet implemented in octopus-cli.");
                return;
            }
            Commands::Vis => {
                println!("Visualizer server is not yet implemented in octopus-cli.");
                return;
            }
        }
    }

    // Main CLI flow
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    runtime.block_on(async_main(cli));
}

fn print_fatal_error(message: &str) {
    eprintln!("{}", message);
}

async fn async_main(cli: Cli) {
    enable_logging(cli.debug, false);

    let work_dir = cli
        .work_dir
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    // Conflict checks
    let mut conflicts = Vec::new();
    if [cli.print, cli.acp, cli.wire]
        .iter()
        .filter(|&&x| x)
        .count()
        > 1
    {
        conflicts.push("Cannot combine --print, --acp, --wire".to_string());
    }
    if cli.agent.is_some() && cli.agent_file.is_some() {
        conflicts.push("Cannot combine --agent and --agent-file".to_string());
    }
    if cli.continue_ && (cli.session.is_some() || cli.session.as_deref() == Some("")) {
        conflicts.push("Cannot combine --continue and --session".to_string());
    }
    if cli.config.is_some() && cli.config_file.is_some() {
        conflicts.push("Cannot combine --config and --config-file".to_string());
    }
    if !conflicts.is_empty() {
        for c in &conflicts {
            print_fatal_error(c);
        }
        process::exit(1);
    }

    // Determine agent file
    let agent_file = if let Some(ref file) = cli.agent_file {
        Some(file.clone())
    } else {
        match cli.agent {
            Some(AgentChoice::Default) => Some(PathBuf::from("agents/default/agent.yaml")),
            Some(AgentChoice::Okabe) => Some(PathBuf::from("agents/okabe/agent.yaml")),
            None => None,
        }
    };
    let _agent_file = agent_file;

    // Parse MCP configs
    let mut mcp_configs: Vec<serde_json::Value> = Vec::new();
    for file in &cli.mcp_config_file {
        match std::fs::read_to_string(file) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(val) => mcp_configs.push(val),
                Err(e) => {
                    eprintln!("Invalid JSON in MCP config file {}: {}", file.display(), e);
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Failed to read MCP config file {}: {}", file.display(), e);
                std::process::exit(1);
            }
        }
    }
    for config_str in &cli.mcp_config {
        match serde_json::from_str(config_str) {
            Ok(val) => mcp_configs.push(val),
            Err(e) => {
                eprintln!("Invalid JSON in MCP config: {}: {}", config_str, e);
                std::process::exit(1);
            }
        }
    }
    let _mcp_configs = mcp_configs;

    // Determine UI mode
    let mut ui_mode = if cli.print {
        "print"
    } else if cli.acp {
        "acp"
    } else if cli.wire {
        "wire"
    } else {
        "shell"
    };

    // Handle quiet mode
    let (print_mode, output_format_val, final_message_only) = if cli.quiet {
        if cli.acp || cli.wire {
            eprintln!("Quiet mode cannot be combined with ACP or Wire UI");
            std::process::exit(1);
        }
        if cli.output_format.is_some() && !matches!(cli.output_format, Some(OutputFormat::Text)) {
            eprintln!("Quiet mode implies --output-format text");
            std::process::exit(1);
        }
        ui_mode = "print";
        (true, Some(OutputFormat::Text), true)
    } else {
        (cli.print, cli.output_format.clone(), cli.final_message_only)
    };

    // Load config
    let config = if let Some(ref config_string) = cli.config {
        match load_config_from_string(config_string) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Invalid config: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let config_path = cli.config_file;

    // Determine session
    let mut session_id = cli.session.clone();
    let picker_mode = session_id.as_deref() == Some("");
    if session_id.as_deref() == Some("") {
        session_id = None;
    }

    let continue_ = cli.continue_;

    // Prompt validation
    let prompt = cli
        .prompt
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if let Some(ref p) = prompt {
        if p.is_empty() {
            eprintln!("Prompt cannot be empty");
            std::process::exit(1);
        }
    }

    // Input/output format validation
    if cli.input_format.is_some() && ui_mode != "print" {
        eprintln!("Input format is only supported for print UI");
        std::process::exit(1);
    }
    if cli.output_format.is_some() && ui_mode != "print" && !cli.quiet {
        eprintln!("Output format is only supported for print UI");
        std::process::exit(1);
    }
    if cli.final_message_only && ui_mode != "print" && !cli.quiet {
        eprintln!("Final-message-only output is only supported for print UI");
        std::process::exit(1);
    }
    if picker_mode && ui_mode != "shell" {
        eprintln!("--session without a session ID is only supported for shell UI");
        std::process::exit(1);
    }

    // Handle picker mode
    if picker_mode {
        let sessions = Session::list(&work_dir).await;
        if sessions.is_empty() {
            println!("No sessions found for the working directory.");
            std::process::exit(0);
        }
        match octopus_cli::ui::picker::pick_session_interactive(sessions) {
            Ok(Some(id)) => {
                session_id = Some(id);
            }
            Ok(None) => {
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Picker error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Main loop with reload support
    let last_session_id = session_id.clone();
    let exit_code;

    loop {
        let session = if let Some(ref sid) = last_session_id {
            match Session::find(&work_dir, sid).await {
                Some(s) => {
                    tracing::info!("Resuming session: {}", sid);
                    s
                }
                None => {
                    tracing::info!("Session {} not found, creating new session", sid);
                    match Session::create(&work_dir, Some(sid.clone())).await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Failed to create session: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
        } else if continue_ {
            match Session::continue_(&work_dir).await {
                Some(s) => {
                    tracing::info!("Continuing previous session: {}", s.id);
                    s
                }
                None => {
                    eprintln!("No previous session found for the working directory");
                    std::process::exit(1);
                }
            }
        } else {
            match Session::create(&work_dir, None).await {
                Ok(s) => {
                    tracing::info!("Created new session: {}", s.id);
                    s
                }
                Err(e) => {
                    eprintln!("Failed to create session: {}", e);
                    std::process::exit(1);
                }
            }
        };

        let resumed = last_session_id.is_some() || continue_;

        let mut instance = match OctopusCLI::create(
            session,
            config.clone(),
            config_path.clone(),
            cli.model.clone(),
            cli.thinking,
            cli.yolo,
            cli.afk,
            cli.plan,
            resumed,
            ui_mode.to_string(),
            cli.max_steps_per_turn,
            cli.max_retries_per_step,
            cli.max_ralph_iterations,
        )
        .await
        {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Failed to initialize Kimi CLI: {}", e);
                std::process::exit(1);
            }
        };

        let result = if print_mode {
            instance
                .run_print(
                    cli.input_format.clone().unwrap_or(InputFormat::Text),
                    output_format_val.clone().unwrap_or(OutputFormat::Text),
                    prompt.clone(),
                    final_message_only,
                )
                .await
        } else if ui_mode == "acp" {
            instance.run_acp().await.map(|_| 0)
        } else if ui_mode == "wire" {
            instance.run_wire_stdio().await.map(|_| 0)
        } else {
            instance
                .run_shell(prompt.clone(), None)
                .await
                .map(|ok| if ok { 0 } else { 1 })
        };

        match result {
            Ok(code) => {
                exit_code = code;
                break;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    std::process::exit(exit_code);
}
