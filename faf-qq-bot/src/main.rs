mod bot;
mod buffer;
mod config;
mod llm;
mod napcat;
mod onebot;

use crate::bot::Bot;
use crate::config::Config;
use crate::napcat::NapcatManager;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "faf-qq-bot")]
#[command(about = "A cloud-deployable QQ group summary bot via NapCatQQ / OneBot 11")]
struct Cli {
    /// Path to the configuration file.
    #[arg(default_value = "config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the bot only (assume NapCatQQ is already running).
    Run,
    /// Create directories and write example NapCatQQ / bot configs.
    Setup,
    /// Start NapCatQQ and then run the bot.
    Start,
    /// Stop the managed NapCatQQ process.
    Stop,
    /// Show NapCatQQ and bot status.
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    info!(config = %cli.config.display(), "loading configuration");
    let config = Config::from_file(&cli.config)?;
    info!("configuration loaded");

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            run_bot(config).await?;
        }
        Command::Setup => {
            setup(config).await?;
        }
        Command::Start => {
            start(config).await?;
        }
        Command::Stop => {
            stop(config).await?;
        }
        Command::Status => {
            status(config).await?;
        }
    }

    Ok(())
}

async fn run_bot(config: Config) -> anyhow::Result<()> {
    info!(onebot_ws = %config.onebot.ws_url, "connecting to OneBot");
    let (mut event_rx, client) =
        onebot::connect(&config.onebot.ws_url, &config.onebot.access_token).await?;

    let bot = Bot::new(config.bot, config.llm, client).into_arc();

    info!("bot is running; press Ctrl+C to stop");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                let bot = bot.clone();
                tokio::spawn(async move {
                    bot.handle_event(event).await;
                });
            }
            _ = &mut shutdown => {
                info!("shutdown signal received; exiting");
                break;
            }
            else => {
                warn!("event channel closed; exiting");
                break;
            }
        }
    }

    Ok(())
}

async fn setup(config: Config) -> anyhow::Result<()> {
    let manager = NapcatManager::new(config.napcat);
    manager.ensure_data_dir().await?;

    info!(
        napcat_dir = %manager.config.dir,
        data_dir = %manager.config.data_dir,
        "setup complete"
    );
    info!("Next steps:");
    info!("  1. Install NapCatQQ into the napcat.dir directory.");
    info!("  2. Edit config.toml with your QQ account, OneBot port, and Kimi API key.");
    info!("  3. Run `faf-qq-bot start` to launch NapCatQQ and the bot.");

    Ok(())
}

async fn start(config: Config) -> anyhow::Result<()> {
    let mut manager = NapcatManager::new(config.napcat.clone());

    manager.start().await?;
    manager.wait_for_onebot(&config.onebot.ws_url, 60).await?;

    run_bot(config).await
}

async fn stop(config: Config) -> anyhow::Result<()> {
    let mut manager = NapcatManager::new(config.napcat);
    manager.stop().await?;
    Ok(())
}

async fn status(config: Config) -> anyhow::Result<()> {
    let mut manager = NapcatManager::new(config.napcat);
    if manager.is_running().await {
        info!("NapCatQQ: running");
    } else {
        info!("NapCatQQ: not running");
    }
    Ok(())
}
