use crate::service::{base_dir, SNOWLUMA_CONTAINER};
use anyhow::Result;
use std::path::Path;
use tokio::process::Command;

pub async fn run(data_dir: &Path) -> Result<()> {
    let base = base_dir(data_dir);
    println!("=== qqbot reset ===");

    // Stop daemon if running.
    if crate::daemon::is_alive(data_dir) {
        println!("Stopping qqbot daemon...");
        crate::daemon::stop(data_dir).await?;
    } else {
        println!("qqbot daemon is not running");
    }

    // Stop and remove SnowLuma container.
    println!("Stopping SnowLuma container...");
    let _ = Command::new("docker")
        .args(["stop", "-t", "10", SNOWLUMA_CONTAINER])
        .output()
        .await;
    println!("Removing SnowLuma container...");
    let _ = Command::new("docker")
        .args(["rm", "-f", SNOWLUMA_CONTAINER])
        .output()
        .await;

    // Remove session data only (preserve configs and plugins).
    let session_dirs = [
        base.join("snowluma-data/.config"),
        base.join("snowluma-data/.local"),
        base.join("snowluma-data/data"),
    ];
    for dir in &session_dirs {
        if dir.exists() {
            println!("Removing session directory: {}", dir.display());
            tokio::fs::remove_dir_all(dir).await?;
        }
    }

    // Remove pid file.
    let pid_file = data_dir.join("run/qqbot.pid");
    if pid_file.exists() {
        tokio::fs::remove_file(&pid_file).await?;
    }

    println!("\nReset complete.");
    println!("Run `qqbot init` to reconfigure, then `qqbot start` to scan the QR code again.");
    Ok(())
}
