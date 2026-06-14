use crate::daemon;
use crate::service::{base_dir, SNOWLUMA_CONTAINER};
use anyhow::Result;
use std::path::Path;
use tokio::net::TcpStream;
use tokio::process::Command;

pub async fn show(data_dir: &Path) -> Result<()> {
    let mut checks: Vec<(String, bool, Option<String>)> = Vec::new();

    // Daemon.
    let daemon_alive = daemon::is_alive(data_dir);
    if daemon_alive {
        if let Some(pid) = daemon::pid(data_dir) {
            checks.push((format!("qqbot daemon running (pid {pid})"), true, None));
        } else {
            checks.push(("qqbot daemon pid file unreadable".to_string(), false, None));
        }
    } else {
        checks.push(("qqbot daemon running".to_string(), false, None));
    }

    // SnowLuma container.
    let container_running = is_container_running().await;
    checks.push((
        "SnowLuma container running".to_string(),
        container_running,
        None,
    ));

    // OneBot WebSocket.
    let ws_reachable = TcpStream::connect(("127.0.0.1", 3001)).await.is_ok();
    checks.push((
        "OneBot WebSocket reachable".to_string(),
        ws_reachable,
        Some("ws://127.0.0.1:3001".to_string()),
    ));

    // qqbot-core process.
    let core_running = is_core_running().await;
    checks.push(("qqbot-core process running".to_string(), core_running, None));

    // Ports.
    for (name, port) in [("SnowLuma WebUI", 5099u16), ("noVNC", 6081)] {
        let reachable = TcpStream::connect(("127.0.0.1", port)).await.is_ok();
        checks.push((format!("{name} port {port} reachable"), reachable, None));
    }

    // Print checklist.
    for (label, ok, detail) in checks {
        let symbol = if ok { "[ok]" } else { "[fail]" };
        let suffix = detail.map(|d| format!(" ({d})")).unwrap_or_default();
        println!("{symbol} {label}{suffix}");
    }

    println!();
    if daemon_alive && container_running && ws_reachable && core_running {
        println!("Status: all systems go.");
    } else {
        println!("Status: some components are not ready.");
        println!("Run `qqbot doctor` for more detail.");
    }

    println!();
    println!("Data directory: {}", base_dir(data_dir).display());
    println!("WebUI:          {}", hyperlink("http://localhost:5099", "http://localhost:5099"));
    println!("noVNC:          {} (password: vncpasswd)", hyperlink("http://localhost:6081", "http://localhost:6081"));

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

async fn is_core_running() -> bool {
    match Command::new("pgrep").args(["-f", "qqbot-core"]).output().await {
        Ok(output) => output.status.success() && !output.stdout.is_empty(),
        Err(_) => false,
    }
}
