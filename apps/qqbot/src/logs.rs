use crate::service::{logs_dir, SNOWLUMA_CONTAINER};
use anyhow::{Context, Result};
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LogTarget {
    Core,
    Snowluma,
    Supervisor,
}

pub async fn tail(data_dir: &Path, target: LogTarget, lines: usize) -> Result<()> {
    match target {
        LogTarget::Core => tail_file(data_dir, "core.log", lines).await,
        LogTarget::Supervisor => tail_file(data_dir, "supervisor.log", lines).await,
        LogTarget::Snowluma => tail_snowluma(lines).await,
    }
}

async fn tail_file(data_dir: &Path, name: &str, lines: usize) -> Result<()> {
    let path = logs_dir(data_dir).join(name);
    if !path.exists() {
        println!("(log file does not exist yet: {})", path.display());
        return Ok(());
    }

    let output = Command::new("tail")
        .args(["-n", &lines.to_string()])
        .arg(&path)
        .output()
        .await
        .context("failed to run tail")?;

    if !output.status.success() {
        anyhow::bail!("tail failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

async fn tail_snowluma(lines: usize) -> Result<()> {
    let output = Command::new("docker")
        .args(["logs", "--tail", &lines.to_string(), SNOWLUMA_CONTAINER])
        .output()
        .await
        .context("failed to run docker logs")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    print!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}
