mod config;
mod core_config;
mod daemon;
mod doctor;
mod health;
mod logs;
mod manager;
mod napcat_config;
mod paths;
mod plugins;
mod reset;
mod service;
mod status;

use crate::config::{Config, CoreConfig, LlmConfig, NapcatConfig, QqConfig};
use crate::core_config::{CoreConfigFile, LlmConfig as CoreLlmConfig};
use crate::manager::{core_config_path, napcat_config_path};
use crate::service::{base_dir, logs_dir, SNOWLUMA_IMAGE};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use napcat_config::OneBot11Config;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;
use tracing::info;

#[derive(Parser)]
#[command(name = "qqbot")]
#[command(about = "QQ bot service manager.")]
struct Cli {
    /// Working directory for qqbot-core config and plugins.
    #[arg(long, short, global = true, default_value = "./data/qqbot-data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// One-shot setup: write configs, ensure SnowLuma image, start daemon.
    Init {
        /// QQ account number for the bot.
        #[arg(long, short)]
        account: i64,
        /// Kimi (Moonshot AI) API key.
        #[arg(long, short)]
        kimi_key: String,
        /// Group IDs the bot is allowed to respond in.
        #[arg(long, short)]
        group: Vec<i64>,
        /// OneBot WebSocket port.
        #[arg(long, default_value_t = 3001)]
        ws_port: u16,
        /// SnowLuma WebUI port.
        #[arg(long, default_value_t = 5099)]
        webui_port: u16,
    },
    /// Start the bot service in the background.
    Start,
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
    Health,
    /// Manage hot-reloadable plugins.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Reset runtime session data (stops services, removes container, clears QQ login).
    Reset,
    /// Low-level setup: write config files only.
    Setup {
        /// QQ account number for the bot.
        #[arg(long, short)]
        account: i64,
        /// Kimi (Moonshot AI) API key.
        #[arg(long, short)]
        kimi_key: String,
        /// Group IDs the bot is allowed to respond in.
        #[arg(long, short)]
        group: Vec<i64>,
        /// NapCatQQ bundle directory.
        #[arg(long, default_value = "./napcat")]
        napcat_dir: String,
        /// NapCatQQ launcher script/binary, relative to napcat_dir.
        #[arg(long, default_value = "napcat.sh")]
        launcher: String,
        /// OneBot WebSocket port.
        #[arg(long, default_value_t = 3001)]
        ws_port: u16,
        /// NapCatQQ WebUI port.
        #[arg(long, default_value_t = 6099)]
        webui_port: u16,
    },
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
    let cli = Cli::parse();
    let data_dir = paths::resolve(&cli.data_dir);

    match cli.command {
        Command::Init {
            account,
            kimi_key,
            group,
            ws_port,
            webui_port,
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
            ))?;
            drop(rt);

            // Daemonize. Parent exits, child continues.
            daemon::start(&data_dir)?;

            // Child process: run the service loop.
            let rt = tokio::runtime::Runtime::new()?;
            let _ = rt.block_on(service::run(&data_dir));
            std::process::exit(0);
        }
        Command::Start => {
            if daemon::is_alive(&data_dir) {
                println!("qqbot daemon is already running");
                return Ok(());
            }
            // Daemonize. Parent exits, child continues.
            daemon::start(&data_dir)?;

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
        Command::Health => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(health::run(&data_dir))?;
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
        Command::Reset => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(reset::run(&data_dir))?;
        }
        Command::Setup {
            account,
            kimi_key,
            group,
            napcat_dir,
            launcher,
            ws_port,
            webui_port,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(setup(
                data_dir, account, kimi_key, group, napcat_dir, launcher, ws_port, webui_port,
            ))?;
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
) -> Result<()> {
    let base = base_dir(&data_dir);
    std::fs::create_dir_all(&base)?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(logs_dir(&data_dir))?;
    std::fs::create_dir_all(data_dir.join("plugins"))?;

    // Write supervisor config.
    let config = Config {
        qq: QqConfig { account },
        napcat: NapcatConfig {
            dir: "./napcat".to_string(),
            launcher: "napcat.sh".to_string(),
            data_dir: data_dir.join("napcat").to_string_lossy().to_string(),
            ws_port,
            webui_port,
        },
        core: CoreConfig {
            binary: "./qqbot-core".to_string(),
            plugin_dir: data_dir.join("plugins").to_string_lossy().to_string(),
            config_path: core_config_path(&data_dir).to_string_lossy().to_string(),
            allowed_groups: groups.clone(),
        },
        llm: LlmConfig {
            api_key: kimi_key.clone(),
            api_url: "https://api.moonshot.cn/v1/chat/completions".to_string(),
            model: "moonshot-v1-8k".to_string(),
        },
    };
    config.to_file(data_dir.join("qqbot.toml"))?;

    // Write qqbot-core config.
    let ws_url = format!("ws://127.0.0.1:{}", ws_port);
    let core_llm = CoreLlmConfig {
        api_url: config.llm.api_url.clone(),
        api_key: config.llm.api_key.clone(),
        model: config.llm.model.clone(),
        system_prompt: "You are a helpful assistant summarizing a QQ group conversation. List key topics, decisions, and action items concisely in the user's language.".to_string(),
    };
    let core_config = CoreConfigFile::new(
        ws_url.clone(),
        data_dir.join("plugins").to_string_lossy().to_string(),
        groups.clone(),
        core_llm,
    );
    core_config.to_file(core_config_path(&data_dir))?;

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
        paths::project_root().join("target/wasm32-unknown-unknown/release/summary.wasm");
    if plugin_src.exists() {
        let plugin_dst = data_dir.join("plugins").join("summary.wasm");
        tokio::fs::copy(&plugin_src, &plugin_dst).await?;
        println!("Copied summary plugin to {}", plugin_dst.display());
    } else {
        println!(
            "Warning: default plugin not found at {}. Build it with:\n  cargo build --release -p summary --target wasm32-unknown-unknown",
            plugin_src.display()
        );
    }

    println!();
    println!("qqbot initialized.");
    println!("Data directory: {}", data_dir.display());
    println!();
    let no_vnc_url = hyperlink("http://localhost:6081", "http://localhost:6081");
    println!("The daemon is starting in the background. Next steps:");
    println!("  1. Wait a few seconds for SnowLuma to start.");
    println!("  2. Open noVNC: {no_vnc_url}");
    println!("     VNC password: vncpasswd");
    println!("  3. Scan the QQ QR code with your phone.");
    println!("  4. Check status: qqbot status");
    println!("  5. View logs:    qqbot logs core -n 50");
    println!();

    Ok(())
}

async fn setup(
    data_dir: PathBuf,
    account: i64,
    kimi_key: String,
    groups: Vec<i64>,
    napcat_dir: String,
    launcher: String,
    ws_port: u16,
    webui_port: u16,
) -> Result<()> {
    let config = Config {
        qq: QqConfig { account },
        napcat: NapcatConfig {
            dir: napcat_dir.clone(),
            launcher,
            data_dir: data_dir.join("napcat").to_string_lossy().to_string(),
            ws_port,
            webui_port,
        },
        core: CoreConfig {
            binary: "./qqbot-core".to_string(),
            plugin_dir: data_dir.join("plugins").to_string_lossy().to_string(),
            config_path: core_config_path(&data_dir).to_string_lossy().to_string(),
            allowed_groups: groups.clone(),
        },
        llm: LlmConfig {
            api_key: kimi_key,
            api_url: "https://api.moonshot.cn/v1/chat/completions".to_string(),
            model: "moonshot-v1-8k".to_string(),
        },
    };

    let manager = manager::ProcessManager::new(config.clone(), data_dir.clone());
    manager.setup().await?;

    // Write supervisor config.
    config.to_file(data_dir.join("qqbot.toml"))?;

    // Write qqbot-core config.
    let ws_url = format!("ws://127.0.0.1:{}", ws_port);
    let core_llm = CoreLlmConfig {
        api_url: config.llm.api_url.clone(),
        api_key: config.llm.api_key.clone(),
        model: config.llm.model.clone(),
        system_prompt: "You are a helpful assistant summarizing a QQ group conversation. List key topics, decisions, and action items concisely in the user's language.".to_string(),
    };
    let core_config = CoreConfigFile::new(
        ws_url.clone(),
        data_dir.join("plugins").to_string_lossy().to_string(),
        groups.clone(),
        core_llm,
    );
    core_config.to_file(core_config_path(&data_dir))?;

    // Write NapCatQQ OneBot config into the NapCatQQ bundle.
    let ob_config = OneBot11Config::with_ws_server(ws_port);
    ob_config.to_file(napcat_config_path(&data_dir, &napcat_dir, account))?;

    info!(data_dir = %data_dir.display(), "setup complete");
    println!("setup complete: {}", data_dir.display());

    Ok(())
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
