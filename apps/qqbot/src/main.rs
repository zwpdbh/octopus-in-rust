mod control;
mod core_config;
mod daemon;
mod doctor;
mod groups;
mod health;
mod llm;
mod logs;
mod oauth;
mod paths;
mod plugins;
mod reset;
mod service;
mod status;

use crate::core_config::CoreConfigFile;
use crate::service::{base_dir, logs_dir, SNOWLUMA_IMAGE};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;
use tracing::info;

#[derive(Parser)]
#[command(name = "qqbot")]
#[command(about = "QQ bot service manager.")]
struct Cli {
    /// Working directory for qqbot-core config and plugins.
    ///
    /// If omitted, qqbot uses the directory recorded by the most recent
    /// `qqbot init` (stored in `<project-root>/.qqbot`) and falls back to
    /// `./data/qqbot-data`.
    #[arg(long, short, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// One-shot setup: write configs, ensure SnowLuma image, start daemon.
    ///
    /// Values can also be provided via a `.env` file or environment variables:
    ///   QQBOT_ACCOUNT, QQBOT_KIMI_KEY, QQBOT_GROUP, QQBOT_WS_PORT, QQBOT_WEBUI_PORT.
    Init {
        /// QQ account number for the bot.
        #[arg(long, short, env = "QQBOT_ACCOUNT")]
        account: i64,
        /// Kimi (Moonshot AI) API key.
        #[arg(long, short, env = "QQBOT_KIMI_KEY")]
        kimi_key: String,
        /// Group IDs the bot is allowed to respond in (comma-separated when set via env).
        #[arg(long, short, env = "QQBOT_GROUP", value_delimiter = ',')]
        group: Vec<i64>,
        /// OneBot WebSocket port.
        #[arg(long, default_value_t = 3001, env = "QQBOT_WS_PORT")]
        ws_port: u16,
        /// SnowLuma WebUI port.
        #[arg(long, default_value_t = 5099, env = "QQBOT_WEBUI_PORT")]
        webui_port: u16,
        /// Reset the SnowLuma WebUI admin password and print the new one-time password.
        /// Use this if you do not know the current WebUI password.
        #[arg(long)]
        reset_webui_password: bool,
    },
    /// Start the bot service in the background.
    Start {
        /// Run the supervisor in the foreground instead of daemonizing.
        /// Useful when running under systemd.
        #[arg(long)]
        no_daemon: bool,
    },
    /// Stop the bot service.
    Stop,
    /// Restart the bot service.
    Restart,
    /// Show service status.
    Status,
    /// Show recent logs.
    Logs {
        #[arg(value_enum, default_value_t = logs::LogTarget::Core)]
        target: logs::LogTarget,
        /// Number of lines to show.
        #[arg(long, short, default_value_t = 50)]
        n: usize,
    },
    /// Run diagnostic checks.
    Doctor,
    /// Check whether the bot is ready to send/receive messages.
    Health {
        /// Group ID to use for the end-to-end echo check. If omitted, the first
        /// allowed group where the bot is a member is used.
        #[arg(long, short)]
        group: Option<i64>,
    },
    /// Manage hot-reloadable plugins.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Test the configured LLM API key and model.
    Llm {
        #[command(subcommand)]
        command: LlmCommand,
    },
    /// Register or inspect WASM plugin tool files directly.
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    /// Manage per-group skills (system prompt and plugin set).
    Group {
        group_id: i64,
        #[command(subcommand)]
        command: GroupCommand,
    },
    /// Reset runtime session data (stops services, removes container, clears QQ login).
    Reset,
}

#[derive(Debug, Clone, Subcommand)]
enum LlmCommand {
    /// Check whether the configured API key can authenticate and whether the model exists.
    Test {
        /// Override the API base URL (e.g. https://api.moonshot.ai/v1).
        #[arg(long, short)]
        base_url: Option<String>,
    },
    /// Send a test prompt to the configured LLM and print the reply.
    Ask {
        /// Prompt to send.
        prompt: Vec<String>,
        /// Override the model from config.toml.
        #[arg(long, short)]
        model: Option<String>,
        /// Override the API base URL (e.g. https://api.moonshot.ai/v1).
        #[arg(long, short)]
        base_url: Option<String>,
    },
    /// Stream a test prompt to the configured LLM and print chunks as they arrive.
    Stream {
        /// Prompt to send.
        prompt: Vec<String>,
        /// Override the model from config.toml.
        #[arg(long, short)]
        model: Option<String>,
        /// Override the API base URL (e.g. https://api.moonshot.ai/v1).
        #[arg(long, short)]
        base_url: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum GroupCommand {
    /// Set the system prompt for this group.
    SetPrompt { prompt: Vec<String> },
    /// Add a plugin to this group's whitelist.
    EnablePlugin { plugin: String },
    /// Show the group's effective profile.
    Show,
}

#[derive(Debug, Clone, Subcommand)]
enum ToolsCommand {
    /// Install or overwrite a built .wasm plugin from an explicit path.
    Register { path: PathBuf },
    /// Update an already installed plugin from a built .wasm path.
    Update { path: PathBuf },
    /// Uninstall a plugin by its file-stem name (e.g. `faf_units_plugin`).
    Unregister { name: String },
    /// List currently registered plugin tools.
    List,
}

#[derive(Debug, Clone, Subcommand)]
enum PluginCommand {
    /// List available and enabled plugins.
    List,
    /// Enable a plugin (copy to plugin dir and reload core).
    Enable { name: String },
    /// Disable a plugin (remove from plugin dir and reload core).
    Disable { name: String },
    /// Signal qqbot-core to reload plugins without restarting.
    Reload,
}

fn main() -> Result<()> {
    // Load `.env` from the current working directory if present. This lets
    // users keep init secrets (API key, QQ account, group IDs) out of shell
    // history and command lines.
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    let data_dir = cli
        .data_dir
        .map(paths::resolve)
        .or_else(paths::read_default_data_dir)
        .unwrap_or_else(|| paths::resolve("./data/qqbot-data"));

    match cli.command {
        Command::Init {
            account,
            kimi_key,
            group,
            ws_port,
            webui_port,
            reset_webui_password,
        } => {
            // Run setup synchronously before daemonizing.
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(init(
                data_dir.clone(),
                account,
                kimi_key,
                group,
                ws_port,
                webui_port,
                reset_webui_password,
            ))?;
            drop(rt);

            // Daemonize. Parent exits, child continues.
            daemon::start(&data_dir)?;

            // Child process: run the service loop.
            let rt = tokio::runtime::Runtime::new()?;
            let _ = rt.block_on(service::run(&data_dir));
            std::process::exit(0);
        }
        Command::Start { no_daemon } => {
            if !no_daemon && daemon::is_alive(&data_dir) {
                println!("qqbot daemon is already running");
                return Ok(());
            }
            if !is_initialized(&data_dir) {
                eprintln!("qqbot is not initialized.");
                eprintln!(
                    "Run: cargo run --bin qqbot -- init --account <ACCOUNT> --kimi-key <KEY>"
                );
                eprintln!("For full options: cargo run --bin qqbot -- init --help");
                std::process::exit(1);
            }

            if !no_daemon {
                // Daemonize. Parent exits, child continues.
                daemon::start(&data_dir)?;
            }

            // Child process: run the service loop.
            let rt = tokio::runtime::Runtime::new()?;
            let _ = rt.block_on(service::run(&data_dir));
            std::process::exit(0);
        }
        Command::Stop => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(daemon::stop(&data_dir))?;
            println!("qqbot daemon stopped");
        }
        Command::Restart => {
            let rt = tokio::runtime::Runtime::new()?;
            let _ = rt.block_on(daemon::stop(&data_dir));
            drop(rt);

            daemon::start(&data_dir)?;
            let rt = tokio::runtime::Runtime::new()?;
            let _ = rt.block_on(service::run(&data_dir));
            std::process::exit(0);
        }
        Command::Status => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(status::show(&data_dir))?;
        }
        Command::Logs { target, n } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(logs::tail(&data_dir, target, n))?;
        }
        Command::Doctor => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(doctor::run(&data_dir))?;
        }
        Command::Health { group } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(health::run(&data_dir, group))?;
        }
        Command::Plugin { command } => match command {
            PluginCommand::List => {
                for p in plugins::list(&data_dir)? {
                    let status = match (p.available, p.enabled) {
                        (true, true) => "enabled",
                        (true, false) => "available",
                        (false, true) => "enabled (source missing)",
                        (false, false) => "unavailable",
                    };
                    println!("{:<20} {}", p.name, status);
                }
            }
            PluginCommand::Enable { name } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(plugins::enable(&data_dir, &name))?;
            }
            PluginCommand::Disable { name } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(plugins::disable(&data_dir, &name))?;
            }
            PluginCommand::Reload => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(plugins::reload(&data_dir))?;
            }
        },
        Command::Group { group_id, command } => match command {
            GroupCommand::SetPrompt { prompt } => {
                let text = prompt.join(" ");
                if text.is_empty() {
                    anyhow::bail!("prompt is required");
                }
                groups::set_prompt(&data_dir, group_id, &text)?;
            }
            GroupCommand::EnablePlugin { plugin } => {
                groups::enable_plugin(&data_dir, group_id, &plugin)?;
            }
            GroupCommand::Show => {
                groups::show(&data_dir, group_id)?;
            }
        },
        Command::Tools { command } => match command {
            ToolsCommand::Register { path } | ToolsCommand::Update { path } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(plugins::register(&data_dir, &path))?;
            }
            ToolsCommand::Unregister { name } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(plugins::unregister(&data_dir, &name))?;
            }
            ToolsCommand::List => {
                let rt = tokio::runtime::Runtime::new()?;
                let runtime_tools = rt.block_on(control::list_runtime_tools(&data_dir));

                println!("Runtime loaded tools:");
                match &runtime_tools {
                    Ok(tools) => {
                        if tools.is_empty() {
                            println!("  (none — ensure qqbot-core is running and check its logs)");
                        } else {
                            for tool in tools {
                                println!("  {:<28} [{}]", tool.name, tool.source);
                            }
                        }
                    }
                    Err(e) => {
                        println!("  unavailable: {e}");
                    }
                }

                println!();
                println!("Installed plugin tools:");
                let installed = plugins::list_registered(&data_dir)?;
                if installed.is_empty() {
                    println!("  (none)");
                } else {
                    for tool in installed {
                        println!("  {:<28} {}", tool.name, tool.description);
                    }
                }
            }
        },
        Command::Llm { command } => match command {
            LlmCommand::Test { base_url } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(llm::test(&data_dir, base_url.as_deref()))?;
            }
            LlmCommand::Ask {
                prompt,
                model,
                base_url,
            } => {
                let text = prompt.join(" ");
                if text.is_empty() {
                    anyhow::bail!("prompt is required");
                }
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(llm::ask(
                    &data_dir,
                    &text,
                    model.as_deref(),
                    base_url.as_deref(),
                ))?;
            }
            LlmCommand::Stream {
                prompt,
                model,
                base_url,
            } => {
                let text = prompt.join(" ");
                if text.is_empty() {
                    anyhow::bail!("prompt is required");
                }
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(llm::stream(
                    &data_dir,
                    &text,
                    model.as_deref(),
                    base_url.as_deref(),
                ))?;
            }
        },
        Command::Reset => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(reset::run(&data_dir))?;
        }
    }

    Ok(())
}

async fn init(
    data_dir: PathBuf,
    account: i64,
    kimi_key: String,
    groups: Vec<i64>,
    ws_port: u16,
    webui_port: u16,
    reset_webui_password: bool,
) -> Result<()> {
    let base = base_dir(&data_dir);

    // If a daemon is already running, stop it so we can re-bind the pid file
    // and take over management of SnowLuma.
    if daemon::is_alive(&data_dir) {
        println!("Stopping existing qqbot daemon...");
        daemon::stop(&data_dir)
            .await
            .context("failed to stop existing qqbot daemon")?;
    }

    std::fs::create_dir_all(&base)?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(logs_dir(&data_dir))?;
    std::fs::create_dir_all(data_dir.join("plugins"))?;

    // Write qqbot-core config.
    let ws_url = format!("ws://127.0.0.1:{}", ws_port);
    let core_llm = CoreConfigFile::default_llm_config(kimi_key);
    let core_config = CoreConfigFile::new(
        ws_url,
        data_dir.join("plugins").to_string_lossy().to_string(),
        groups.clone(),
        account,
        core_llm,
    );
    core_config.to_file(data_dir.join("config.toml"))?;

    // Write SnowLuma OneBot config.
    let snowluma_config_dir = base.join("snowluma-data").join("config");
    std::fs::create_dir_all(&snowluma_config_dir)?;
    std::fs::write(
        snowluma_config_dir.join("onebot.json"),
        service::default_snowluma_onebot_config(),
    )?;

    // Pull SnowLuma image.
    println!("Pulling SnowLuma Docker image...");
    let pull = TokioCommand::new("docker")
        .args(["pull", SNOWLUMA_IMAGE])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to pull SnowLuma image")?;
    if !pull.success() {
        anyhow::bail!("failed to pull SnowLuma image");
    }

    // Copy default plugin if built.
    let plugin_src =
        paths::project_root().join("target/wasm32-unknown-unknown/release/faf_units_plugin.wasm");
    if plugin_src.exists() {
        let plugin_dst = data_dir.join("plugins").join("faf_units_plugin.wasm");
        tokio::fs::copy(&plugin_src, &plugin_dst).await?;
        println!("Copied faf-units plugin to {}", plugin_dst.display());
    } else {
        println!(
            "Warning: default plugin not found at {}. Build it with:\n  cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown",
            plugin_src.display()
        );
    }

    // Optionally reset the SnowLuma WebUI password so SnowLuma prints a new
    // one-time password on the next start.
    let webui_password_file = base.join("snowluma-data").join("config").join("webui.json");
    let webui_existed = webui_password_file.exists();
    if reset_webui_password {
        if webui_existed {
            println!("Resetting SnowLuma WebUI password...");
            tokio::fs::remove_file(&webui_password_file).await?;
        }
        if service::container_running().await.unwrap_or(false) {
            println!("Stopping existing SnowLuma container to apply password reset...");
            service::stop_snowluma()
                .await
                .context("failed to stop SnowLuma container")?;
        }
    }

    // Start SnowLuma now so the user can scan the QR code right away.
    println!("Starting SnowLuma container...");
    service::start_snowluma(&base)
        .await
        .context("failed to start SnowLuma container")?;

    // Wait for the services to be reachable before printing the guide.
    println!("Waiting for SnowLuma services...");
    let _ = service::wait_for_port("127.0.0.1", 5099, 30).await;
    let _ = service::wait_for_port("127.0.0.1", 6081, 30).await;
    let _ = service::wait_for_port("127.0.0.1", 3001, 30).await;

    // Show the WebUI one-time password only when it is genuinely a fresh
    // SnowLuma start (either the data was empty or the user asked to reset it).
    let show_webui_password = reset_webui_password || !webui_existed;
    if show_webui_password {
        match service::extract_snowluma_webui_password().await {
            Some(password) => {
                println!("SnowLuma WebUI one-time password (first login only, username: admin): {password}");
            }
            None => {
                println!("SnowLuma WebUI username: admin");
                println!("The new one-time password could not be read from the container logs.");
                println!("You can find it with:");
                println!("  docker logs snowluma 2>&1 | grep -E \"initial credentials|临时密码\"");
            }
        }
    } else {
        println!("SnowLuma WebUI username: admin");
        println!("Existing WebUI password preserved. If you forgot it, re-run init with --reset-webui-password.");
    }

    // Remember this data directory so later commands do not need `-d`.
    if let Err(e) = paths::write_default_data_dir(&data_dir) {
        eprintln!(
            "Warning: could not write default data directory marker: {e}\nYou may need to pass -d on future commands."
        );
    }

    info!(webui_port = webui_port, "qqbot initialized");
    println!();
    println!("qqbot initialized.");
    println!("Data directory: {}", data_dir.display());
    println!();
    print_init_guide(account, &groups, &data_dir);
    println!();

    Ok(())
}

fn is_initialized(data_dir: &std::path::Path) -> bool {
    let config = data_dir.join("config.toml");
    let base = base_dir(data_dir);
    let onebot_config = base.join("snowluma-data/config/onebot.json");
    config.exists() && onebot_config.exists()
}

fn print_init_guide(account: i64, groups: &[i64], data_dir: &std::path::Path) {
    let no_vnc_url = hyperlink("http://localhost:6081", "http://localhost:6081");
    let webui_url = hyperlink("http://localhost:5099", "http://localhost:5099");
    let config_path = data_dir.join("config.toml");

    println!("The daemon is starting in the background. Complete these steps:");
    println!();
    println!("  1. Open noVNC and scan the QQ QR code:");
    println!("     {no_vnc_url}");
    println!("     VNC password: vncpasswd");
    println!();
    println!("  2. Use your phone's QQ app to scan the QR code in the noVNC window.");
    println!("     This logs in the bot account: {account}");
    println!();
    println!("  3. Add the bot account ({account}) to the QQ group(s) it should monitor.");
    println!();
    if groups.is_empty() {
        println!("  4. Allow those groups in the bot config:");
        println!("     Edit {}", config_path.display());
        println!("     Set allowed_groups = [123456789]  (replace with your group ID(s))");
        println!("     Then run: cargo run --bin qqbot -- restart");
    } else {
        println!("  4. The bot is configured to respond in these groups:");
        println!("     allowed_groups = {groups:?}");
        println!("     Make sure the bot account is a member of each group.");
    }
    println!();
    println!("  5. Wait for the OneBot WebSocket handshake. Run:");
    println!("     cargo run --bin qqbot -- status");
    println!();
    println!("  6. View logs:");
    println!("     cargo run --bin qqbot -- logs core -n 50");
    println!();
    println!("SnowLuma WebUI (optional): {webui_url}  (username: admin)");
}

fn hyperlink(url: &str, text: &str) -> String {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        // OSC 8 hyperlink escape sequence. Terminals that support it make the
        // text clickable and open the URL in the default browser.
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        text.to_string()
    }
}
