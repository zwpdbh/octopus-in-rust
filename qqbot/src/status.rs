use crate::daemon;
use crate::service::{base_dir, SNOWLUMA_CONTAINER};
use anyhow::Result;
use std::path::Path;
use tokio::net::TcpStream;
use tokio::process::Command;

pub async fn show(data_dir: &Path) -> Result<()> {
    let daemon_alive = daemon::is_alive(data_dir);
    let daemon_pid = daemon::pid(data_dir);

    let snowluma_running = is_container_running().await;
    let ws_reachable = TcpStream::connect(("127.0.0.1", 3001)).await.is_ok();

    println!("qqbot daemon:");
    match daemon_pid {
        Some(pid) => println!("  pid: {} ({})", pid, if daemon_alive { "alive" } else { "dead" }),
        None => println!("  not running"),
    }

    println!("SnowLuma container:");
    println!(
        "  {}",
        if snowluma_running {
            "running"
        } else {
            "not running"
        }
    );

    println!("OneBot WebSocket:");
    println!(
        "  {}",
        if ws_reachable {
            "reachable (ws://127.0.0.1:3001)"
        } else {
            "not reachable"
        }
    );

    println!("qqbot-core:");
    if daemon_alive && snowluma_running && ws_reachable {
        println!("  expected to be running (managed by daemon)");
    } else {
        println!("  not ready");
    }

    println!();
    println!("Data directory: {}", base_dir(data_dir).display());
    println!("WebUI:          http://localhost:5099");
    println!("noVNC:          http://localhost:6081 (password: vncpasswd)");

    Ok(())
}

async fn is_container_running() -> bool {
    match Command::new("docker")
        .args(["ps", "-q", "-f", &format!("name={SNOWLUMA_CONTAINER}")])
        .output()
        .await
    {
        Ok(output) => !output.stdout.is_empty(),
        Err(_) => false,
    }
}
