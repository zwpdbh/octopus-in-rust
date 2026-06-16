use crate::core_config::CoreConfigFile;
use crate::daemon;
use crate::health;
use crate::service::{base_dir, SNOWLUMA_CONTAINER};
use anyhow::Result;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
struct Check {
    label: String,
    state: CheckState,
    detail: Option<String>,
    hint: Option<String>,
}

impl Check {
    fn ok(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            state: CheckState::Ok,
            detail: None,
            hint: None,
        }
    }

    fn warn(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            state: CheckState::Warn,
            detail: None,
            hint: Some(hint.into()),
        }
    }

    fn fail(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            state: CheckState::Fail,
            detail: None,
            hint: Some(hint.into()),
        }
    }

    fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

pub async fn show(data_dir: &Path) -> Result<()> {
    let mut checks: Vec<Check> = Vec::new();
    let mut hints: Vec<String> = Vec::new();

    // Daemon.
    let daemon_alive = daemon::is_alive(data_dir);
    if daemon_alive {
        if let Some(pid) = daemon::pid(data_dir) {
            checks.push(Check::ok(format!("qqbot daemon running (pid {pid})")));
        } else {
            checks.push(Check::warn(
                "qqbot daemon pid file unreadable",
                "Run `qqbot start` if the daemon should be running.",
            ));
        }
    } else {
        checks.push(Check::fail(
            "qqbot daemon running",
            "Run `qqbot start` to start the service daemon.",
        ));
    }

    // SnowLuma container.
    let container_running = is_container_running().await;
    checks.push(if container_running {
        Check::ok("SnowLuma container running")
    } else {
        Check::fail(
            "SnowLuma container running",
            "Run `qqbot start` to start SnowLuma and the daemon.",
        )
    });

    // OneBot WebSocket (TCP + handshake).
    let ws_tcp_ok = TcpStream::connect(("127.0.0.1", 3001)).await.is_ok();
    let ws_handshake_ok = ws_handshake().await;
    checks.push(match (ws_tcp_ok, ws_handshake_ok) {
        (true, true) => Check::ok("OneBot WebSocket reachable")
            .with_detail("ws://127.0.0.1:3001"),
        (true, false) => Check::warn(
            "OneBot WebSocket port open but handshake failed",
            "SnowLuma is still starting or QQ is not logged in. Wait a few seconds, then run `qqbot status` again. If it persists, run `qqbot restart`.",
        )
        .with_detail("ws://127.0.0.1:3001"),
        (false, _) => Check::fail(
            "OneBot WebSocket reachable",
            "SnowLuma WebSocket port is closed. Run `qqbot restart` or check `qqbot logs supervisor -n 50`.",
        )
        .with_detail("ws://127.0.0.1:3001"),
    });

    // qqbot-core process.
    let core_running = is_core_running().await;
    checks.push(if core_running {
        Check::ok("qqbot-core process running")
    } else {
        Check::fail(
            "qqbot-core process running",
            "Run `qqbot restart` to restart qqbot-core. If the daemon is not running, use `qqbot start`.",
        )
    });

    // Ports.
    for (name, port) in [("SnowLuma WebUI", 5099u16), ("noVNC", 6081)] {
        let reachable = TcpStream::connect(("127.0.0.1", port)).await.is_ok();
        checks.push(if reachable {
            Check::ok(format!("{name} port {port} reachable"))
        } else {
            Check::warn(
                format!("{name} port {port} not reachable"),
                format!("{name} is not reachable yet. If SnowLuma just started, wait a few seconds. Otherwise run `qqbot restart`."),
            )
        });
    }

    // Print checklist.
    for check in &checks {
        let symbol = match check.state {
            CheckState::Ok => "[ok]",
            CheckState::Warn => "[warn]",
            CheckState::Fail => "[fail]",
        };
        let suffix = check
            .detail
            .as_ref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        println!("{symbol} {}{suffix}", check.label);
        if let Some(hint) = &check.hint {
            hints.push(hint.clone());
        }
    }

    // Application-level health summary.
    let infra_ok =
        daemon_alive && container_running && ws_tcp_ok && ws_handshake_ok && core_running;
    if infra_ok {
        match CoreConfigFile::from_file(data_dir.join("config.toml")) {
            Ok(config) => {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    health::check(data_dir, &config, false),
                )
                .await
                {
                    Ok(Ok(report)) => {
                        if report.online
                            && report.bot_user_id.is_some()
                            && report.group_membership.iter().all(|g| g.member)
                        {
                            println!("[ok] Bot is online and in the allowed group(s)");
                            if let (Some(uid), Some(nick)) =
                                (report.bot_user_id, report.bot_nickname)
                            {
                                println!("       Logged in as {nick} ({uid})");
                            }
                        } else if !report.online {
                            println!("[fail] QQ account is not online");
                            hints.push(
                                "The bot account is offline. Open the SnowLuma WebUI (http://localhost:5099) to log in or scan the QR code.".to_string(),
                            );
                        } else if report.bot_user_id.is_none() {
                            println!("[fail] Could not determine bot user id");
                            hints.push(
                                "OneBot did not report login info. Check `qqbot logs core -n 50` and `qqbot doctor`.".to_string(),
                            );
                        } else {
                            for gm in &report.group_membership {
                                if !gm.member {
                                    println!("[fail] Bot is not a member of allowed group {} — add it to the group first", gm.group_id);
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        println!("[warn] Health check failed: {e}");
                        println!();
                        println!("       What this means:");
                        println!(
                            "       qqbot could not log in through the OneBot WebSocket to verify"
                        );
                        println!("       that the bot can send and receive group messages. The checklist");
                        println!("       above may look healthy while OneBot itself is still starting or");
                        println!("       rejecting connections.");
                        println!();
                        println!("       How to debug:");
                        println!("         - qqbot doctor       (full infrastructure diagnosis)");
                        println!("         - qqbot logs core -n 50");
                        println!("         - qqbot logs supervisor -n 50");
                        println!();
                        println!("       How to fix:");
                        println!("         - Wait a few seconds if SnowLuma just started, then re-run status.");
                        println!("         - qqbot restart      (restart only qqbot-core)");
                        println!(
                            "         - qqbot restart           (restart SnowLuma + qqbot-core)"
                        );
                        hints.push(format!(
                            "Health-check error: {e}. See the detailed explanation above."
                        ));
                    }
                    Err(_) => {
                        println!("[warn] Health check timed out");
                        println!();
                        println!("       The OneBot API did not respond within 10 seconds.");
                        println!("       This usually means SnowLuma is still initializing or QQ is not logged in.");
                        println!();
                        println!("       How to debug:");
                        println!("         - qqbot doctor");
                        println!("         - qqbot logs supervisor -n 50");
                        println!();
                        println!("       How to fix:");
                        println!("         - Wait a few seconds and run `qqbot status` again.");
                        println!("         - qqbot restart");
                        hints.push(
                            "Health check timed out. Wait a few seconds or run `qqbot restart`.".to_string(),
                        );
                    }
                }
            }
            Err(e) => {
                println!("[warn] Could not read config for health check: {e}");
                hints.push(
                    "Could not read qqbot-core config. Run `qqbot init` or check the data directory."
                        .to_string(),
                );
            }
        }
    }

    println!();
    if infra_ok && hints.is_empty() {
        println!("Status: all systems go.");
    } else if infra_ok {
        println!("Status: running, but there are warnings to review.");
    } else {
        println!("Status: some components are not ready.");
    }

    if !hints.is_empty() {
        println!();
        println!("Troubleshooting:");
        for hint in hints {
            println!("  - {hint}");
        }
        println!();
        println!("For a full diagnosis run `qqbot doctor`.");
    }

    println!();
    println!("Data directory: {}", base_dir(data_dir).display());
    println!(
        "WebUI:          {}",
        hyperlink("http://localhost:5099", "http://localhost:5099")
    );
    println!(
        "noVNC:          {} (password: vncpasswd)",
        hyperlink("http://localhost:6081", "http://localhost:6081")
    );

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
    match Command::new("pgrep")
        .args(["-f", "qqbot-core"])
        .output()
        .await
    {
        Ok(output) => output.status.success() && !output.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Perform a minimal WebSocket handshake on the OneBot port. This catches the
/// common case where SnowLuma has opened the TCP port but is not yet accepting
/// WebSocket connections (e.g. QQ is still logging in).
async fn ws_handshake() -> bool {
    let mut stream = match TcpStream::connect(("127.0.0.1", 3001)).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    let key = base64_encode(b"1234567890123456");
    let req = format!(
        "GET / HTTP/1.1\r\n\
         Host: 127.0.0.1:3001\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );

    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }

    let mut buf = [0u8; 1024];
    let n = match tokio::time::timeout(std::time::Duration::from_secs(3), stream.read(&mut buf))
        .await
    {
        Ok(Ok(n)) => n,
        _ => return false,
    };

    let response = String::from_utf8_lossy(&buf[..n]);
    response.starts_with("HTTP/1.1 101")
}

fn base64_encode(input: &[u8]) -> String {
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
