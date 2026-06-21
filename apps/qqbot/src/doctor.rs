use crate::daemon;
use crate::paths;
use crate::service::{base_dir, SNOWLUMA_CONTAINER, SNOWLUMA_IMAGE};
use anyhow::Result;
use std::path::Path;
use tokio::net::TcpStream;
use tokio::process::Command;

pub async fn run(data_dir: &Path) -> Result<()> {
    let base = base_dir(data_dir);
    let mut issues = 0;

    println!("=== qqbot doctor ===\n");

    // Docker daemon.
    match docker_ok().await {
        Ok(true) => println!("[ok] Docker daemon is reachable"),
        Ok(false) => {
            println!("[fail] Docker daemon is not reachable");
            issues += 1;
        }
        Err(e) => {
            println!("[fail] Docker check error: {e}");
            issues += 1;
        }
    }

    // SnowLuma image.
    match image_present().await {
        Ok(true) => println!("[ok] SnowLuma Docker image present ({SNOWLUMA_IMAGE})"),
        Ok(false) => {
            println!("[fail] SnowLuma Docker image not present; run `qqbot init` or `docker pull {SNOWLUMA_IMAGE}`");
            issues += 1;
        }
        Err(e) => {
            println!("[fail] SnowLuma image check error: {e}");
            issues += 1;
        }
    }

    // Binaries.
    let profile = current_profile();
    let binary_dir = binary_search_dir(profile);
    let core_binary = binary_dir.join("qqbot-core");
    if core_binary.exists() {
        println!("[ok] qqbot-core binary found ({})", core_binary.display());
    } else {
        println!("[fail] qqbot-core binary not found; build with `cargo build -p qqbot-core`");
        issues += 1;
    }

    let plugin = data_dir.join("plugins/faf_units_plugin.wasm");
    let plugin = if plugin.exists() {
        plugin
    } else {
        paths::project_root().join("target/wasm32-unknown-unknown/release/faf_units_plugin.wasm")
    };
    if plugin.exists() {
        println!("[ok] faf-units plugin found ({})", plugin.display());
    } else {
        println!("[warn] faf-units plugin not found; build with `cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown`");
    }

    // Config files.
    let config = data_dir.join("config.toml");
    if config.exists() {
        println!("[ok] qqbot-core config found ({})", config.display());
    } else {
        println!("[fail] qqbot-core config not found; run `qqbot init`");
        issues += 1;
    }

    let onebot = base.join("snowluma-data/config/onebot.json");
    if onebot.exists() {
        println!("[ok] SnowLuma OneBot config found ({})", onebot.display());
    } else {
        println!("[warn] SnowLuma OneBot config not found; it will be created on first start");
    }

    // Daemon.
    let daemon_alive = daemon::is_alive(data_dir) || systemd_service_active().await;
    if daemon_alive {
        if let Some(pid) = daemon::pid(data_dir) {
            println!("[ok] qqbot daemon is running (pid {pid})");
        } else if systemd_service_active().await {
            println!("[ok] qqbot systemd service is active");
        } else {
            println!("[warn] daemon pid file is present but unreadable");
        }
    } else {
        println!("[warn] qqbot daemon is not running");
    }

    // SnowLuma container.
    match container_running().await {
        Ok(true) => println!("[ok] SnowLuma container is running"),
        Ok(false) => {
            println!("[warn] SnowLuma container is not running");
        }
        Err(e) => {
            println!("[fail] SnowLuma container check error: {e}");
            issues += 1;
        }
    }

    // Ports.
    for port in [3001u16, 5099, 6081] {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(_) => println!("[ok] port {port} is reachable"),
            Err(_) => println!("[info] port {port} is not reachable"),
        }
    }

    // WebSocket handshake.
    match ws_handshake().await {
        Ok(true) => println!("[ok] OneBot WebSocket handshake succeeded"),
        Ok(false) => {
            println!("[info] OneBot WebSocket handshake failed (QQ may not be logged in yet)")
        }
        Err(e) => println!("[info] OneBot WebSocket check error: {e}"),
    }

    println!();
    if issues == 0 {
        println!("No critical issues found.");
    } else {
        println!("Found {issues} critical issue(s).");
    }

    Ok(())
}

async fn docker_ok() -> Result<bool> {
    let output = Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .await?;
    Ok(output.status.success())
}

async fn image_present() -> Result<bool> {
    let output = Command::new("docker")
        .args(["images", "-q", SNOWLUMA_IMAGE])
        .output()
        .await?;
    Ok(!output.stdout.is_empty())
}

async fn container_running() -> Result<bool> {
    let output = Command::new("docker")
        .args(["ps", "-q", "-f", &format!("name={SNOWLUMA_CONTAINER}")])
        .output()
        .await?;
    Ok(!output.stdout.is_empty())
}

async fn systemd_service_active() -> bool {
    match Command::new("systemctl")
        .args(["is-active", "--quiet", "qqbot"])
        .output()
        .await
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

async fn ws_handshake() -> Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(("127.0.0.1", 3001)).await?;
    let key = base64::encode(b"1234567890123456");
    let req = format!(
        "GET / HTTP/1.1\r\n\
         Host: 127.0.0.1:3001\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let mut buf = [0u8; 1024];
    let n =
        tokio::time::timeout(std::time::Duration::from_secs(3), stream.read(&mut buf)).await??;
    let response = String::from_utf8_lossy(&buf[..n]);
    Ok(response.starts_with("HTTP/1.1 101"))
}

fn current_profile() -> &'static str {
    let exe = std::env::current_exe().unwrap_or_default();
    if exe.to_string_lossy().contains("/debug/") {
        "debug"
    } else {
        "release"
    }
}

/// Directory where we expect to find the qqbot-core binary.
/// In the installed layout it lives next to the qqbot binary; in the dev
/// layout it is under target/{profile}.
fn binary_search_dir(profile: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    if let Some(dir) = exe.parent() {
        let dir_name = dir.file_name().and_then(|n| n.to_str());
        if matches!(dir_name, Some("debug") | Some("release")) {
            if let Some(root) = dir.parent().and_then(|p| p.parent()) {
                return root.join(format!("target/{profile}"));
            }
        }
        // Installed layout: binary is next to qqbot.
        return dir.to_path_buf();
    }
    paths::project_root().join(format!("target/{profile}"))
}

mod base64 {
    pub fn encode(input: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b = match chunk.len() {
                1 => [chunk[0], 0, 0],
                2 => [chunk[0], chunk[1], 0],
                _ => [chunk[0], chunk[1], chunk[2]],
            };
            out.push(TABLE[(b[0] >> 2) as usize] as char);
            out.push(TABLE[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[(b[2] & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        out
    }
}
