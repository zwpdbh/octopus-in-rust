mod config;
mod core_config;
mod manager;
mod napcat_config;

use clap::{Parser, Subcommand};
use config::{Config, CoreConfig, LlmConfig, NapcatConfig, QqConfig};
use core_config::{CoreConfigFile, LlmConfig as CoreLlmConfig};
use manager::{core_config_path, napcat_config_path, ProcessManager};
use napcat_config::OneBot11Config;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
#[command(name = "qqbot")]
#[command(about = "Configure and run a QQ bot with NapCatQQ.")]
struct Cli {
    /// Working directory for configuration, data, and logs.
    #[arg(long, short, global = true, default_value = "./qqbot-data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prepare directories, config files, and NapCatQQ config.
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
    /// Start NapCatQQ and qqbot-core.
    Start,
    /// Stop NapCatQQ and qqbot-core.
    Stop,
    /// Show whether NapCatQQ and qqbot-core are running.
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Setup {
            account,
            kimi_key,
            group,
            napcat_dir,
            launcher,
            ws_port,
            webui_port,
        } => {
            setup(
                cli.data_dir,
                account,
                kimi_key,
                group,
                napcat_dir,
                launcher,
                ws_port,
                webui_port,
            )
            .await?;
        }
        Command::Start => {
            start(cli.data_dir).await?;
        }
        Command::Stop => {
            stop(cli.data_dir).await?;
        }
        Command::Status => {
            status(cli.data_dir).await?;
        }
    }

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
) -> anyhow::Result<()> {
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

    let manager = ProcessManager::new(config.clone(), data_dir.clone());
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
    info!("Next steps:");
    info!(
        "  1. Ensure NapCatQQ bundle is at: {} (launcher: {})",
        napcat_dir, config.napcat.launcher
    );
    info!(
        "  2. Copy your plugins (e.g. summary.wasm) into: {}",
        data_dir.join("plugins").display()
    );
    info!("  3. Run: qqbot start --data-dir {}", data_dir.display());
    info!("  4. Open the NapCatQQ WebUI to scan the QR code.");

    Ok(())
}

async fn start(data_dir: PathBuf) -> anyhow::Result<()> {
    let config = load_config(&data_dir)?;
    let mut manager = ProcessManager::new(config, data_dir);

    manager.start_napcat().await?;
    manager.start_core().await?;

    let (napcat, core) = manager.status().await;
    info!(napcat, core, "processes started");

    // Keep the supervisor alive until Ctrl+C.
    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");
    manager.stop().await?;

    Ok(())
}

async fn stop(data_dir: PathBuf) -> anyhow::Result<()> {
    let config = load_config(&data_dir)?;
    let mut manager = ProcessManager::new(config, data_dir);
    manager.stop().await?;
    Ok(())
}

async fn status(data_dir: PathBuf) -> anyhow::Result<()> {
    let config = load_config(&data_dir)?;
    let mut manager = ProcessManager::new(config, data_dir);
    let (napcat, core) = manager.status().await;
    info!(napcat, core, "process status");
    println!(
        "NapCatQQ: {}",
        if napcat { "running" } else { "not running" }
    );
    println!(
        "qqbot-core: {}",
        if core { "running" } else { "not running" }
    );
    Ok(())
}

fn load_config(data_dir: &PathBuf) -> anyhow::Result<Config> {
    let path = data_dir.join("qqbot.toml");
    if !path.exists() {
        anyhow::bail!(
            "config not found: {}. Run `qqbot setup` first.",
            path.display()
        );
    }
    Config::from_file(&path)
}
