use crate::control;
use crate::core_config::CoreConfigFile;
use crate::daemon;
use crate::health;
use crate::plugins;
use crate::service::{base_dir, SNOWLUMA_CONTAINER};
use anyhow::Result;
use brain::control::{GroupRuntimeStatus, ToolRuntimeInfo};
use std::io::IsTerminal;
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

/// Render `qqbot status` in three sections:
///
/// 1. Global system status
/// 2. Per-group status (configured groups + tools loaded per group)
/// 3. Troubleshooting hints + static info (data dir, WebUI, noVNC)
pub async fn show(data_dir: &Path) -> Result<()> {
    let mut global_hints: Vec<String> = Vec::new();
    let term = TerminalStyle::new();

    // ------------------------------------------------------------------
    // Section 1: Global Systems
    // ------------------------------------------------------------------
    println!("{}", term.section("Global Systems"));

    let mut global_checks: Vec<Check> = Vec::new();

    // Daemon.
    let daemon_alive = daemon::is_alive(data_dir) || systemd_service_active().await;
    if daemon_alive {
        if let Some(pid) = daemon::pid(data_dir) {
            global_checks.push(Check::ok(format!("qqbot daemon running (pid {pid})")));
        } else if systemd_service_active().await {
            global_checks.push(Check::ok("qqbot systemd service active".to_string()));
        } else {
            global_checks.push(Check::warn(
                "qqbot daemon pid file unreadable",
                "Run `qqbot start` if the daemon should be running.",
            ));
        }
    } else {
        global_checks.push(Check::fail(
            "qqbot daemon running",
            "Run `qqbot start` to start the service daemon.",
        ));
    }

    // SnowLuma container.
    let container_running = is_container_running().await;
    global_checks.push(if container_running {
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
    global_checks.push(match (ws_tcp_ok, ws_handshake_ok) {
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
    global_checks.push(if core_running {
        Check::ok("qqbot-core process running")
    } else {
        Check::fail(
            "qqbot-core process running",
            "Run `qqbot restart` to restart qqbot-core. If the daemon is not running, use `qqbot start`.",
        )
    });

    // Detect profile mismatch between CLI and running core.
    if core_running {
        if let Some(mismatch) = core_profile_mismatch(data_dir) {
            global_checks.push(Check::warn(
                format!("Core binary profile mismatch: {mismatch}"),
                "Stop the daemon and restart from the same profile you are running. Example: `./target/release/qqbot stop && ./target/release/qqbot start` or `cargo run --bin qqbot -- start`.",
            ));
        }
    }

    // Available plugin binaries built in the workspace.
    match plugins::available_plugins() {
        Ok(available) => {
            if available.is_empty() {
                global_checks.push(Check::warn(
                    "No plugin binaries built",
                    "Build a plugin with `cargo build --target wasm32-unknown-unknown --release -p <plugin-name>` or run the project build script.",
                ));
            } else {
                global_checks.push(
                    Check::ok(format!(
                        "Available plugins: {}",
                        available.iter().cloned().collect::<Vec<_>>().join(", ")
                    ))
                    .with_detail(format!("{} plugin(s)", available.len())),
                );
            }
        }
        Err(e) => {
            global_checks.push(Check::warn(
                format!("Could not list available plugins: {e}"),
                "Check that the project was built for wasm32-unknown-unknown.",
            ));
        }
    }

    // Registered / loaded plugin tools (global view).
    let runtime_tools_result = control::list_runtime_tools(data_dir).await;
    match &runtime_tools_result {
        Ok(tools) => {
            if tools.is_empty() {
                global_checks.push(Check::warn(
                    "No plugin tools loaded in running core",
                    "Use `qqbot tools register <path>` to install a plugin, then restart qqbot-core or send SIGHUP to load it.",
                ));
            } else {
                // Print a summary check plus the tools grouped by source plugin.
                let total = tools.len();
                let sources = group_tools_by_source(tools);
                println!(
                    "{} Runtime tools loaded ({})",
                    term.ok("[ok]"),
                    term.dim(&format!(
                        "{} tool(s) from {} source(s)",
                        total,
                        sources.len()
                    ))
                );
                for (source, names) in &sources {
                    println!(
                        "       {} {}: {}",
                        term.dim("→"),
                        term.bold(source),
                        names.join(", ")
                    );
                }
            }
        }
        Err(runtime_err) => {
            match plugins::list_registered(data_dir) {
                Ok(tools) => {
                    if tools.is_empty() {
                        global_checks.push(Check::warn(
                        "No plugin tools installed",
                        "Use `qqbot tools register <path>` to load a WASM plugin. The host tool qqbot_recent_messages is always available.",
                    ));
                    } else {
                        let names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
                        global_checks.push(
                        Check::warn(
                            format!("Plugin tools installed (core not reachable: {runtime_err}): {}", names.join(", ")),
                            "Start qqbot-core to load these tools into the runtime.",
                        )
                        .with_detail(format!("{} tool(s)", tools.len())),
                    );
                    }
                }
                Err(e) => {
                    global_checks.push(Check::warn(
                    format!("Could not list plugin tools: runtime {runtime_err}; installed {e}"),
                    "Check that plugin_dir in config.toml points to a readable directory and qqbot-core is running.",
                ));
                }
            }
        }
    }

    // Ports.
    for (name, port) in [("SnowLuma WebUI", 5099u16), ("noVNC", 6081)] {
        let reachable = TcpStream::connect(("127.0.0.1", port)).await.is_ok();
        global_checks.push(if reachable {
            Check::ok(format!("{name} port {port} reachable"))
        } else {
            Check::warn(
                format!("{name} port {port} not reachable"),
                format!("{name} is not reachable yet. If SnowLuma just started, wait a few seconds. Otherwise run `qqbot restart`."),
            )
        });
    }

    for check in &global_checks {
        print_check(check, &mut global_hints);
    }

    // ------------------------------------------------------------------
    // Section 2: Group Status
    // ------------------------------------------------------------------
    println!();
    println!("{}", term.section("Group Status"));

    let config = CoreConfigFile::from_file(data_dir.join("config.toml"));
    let allowed_groups = config
        .as_ref()
        .map(|c| c.bot.allowed_groups.clone())
        .unwrap_or_default();

    if allowed_groups.is_empty() {
        println!("{} No allowed groups configured", term.warn("[warn]"));
        println!("       {} Edit config.toml and add group ids to bot.allowed_groups, then run `qqbot restart`.", term.dim("→"));
        global_hints.push(
            "No allowed groups configured. Update bot.allowed_groups in config.toml.".to_string(),
        );
    } else if !core_running {
        println!(
            "{} qqbot-core is not running; group tool status unavailable",
            term.info("[info]")
        );
        println!(
            "       {} Configured groups: {}",
            term.dim("→"),
            allowed_groups
                .iter()
                .map(|g| g.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        global_hints.push(
            "Start qqbot-core to load Brains and tools for the configured groups.".to_string(),
        );
    } else {
        // Try the control socket first; fall back to installed plugins if it fails.
        let group_statuses: Vec<GroupRuntimeStatus> = match control::group_status(data_dir).await {
            Ok(groups) => groups,
            Err(e) => {
                // We know the core process is running but the socket is unreachable.
                // Show configured groups with whatever info we can gather.
                let is_old_core = e.to_string().contains("unknown variant");
                if is_old_core {
                    println!(
                        "{} Running qqbot-core does not support per-group status queries.",
                        term.warn("[warn]")
                    );
                    println!(
                        "       {} The core binary is older than this CLI. Rebuild and restart it:",
                        term.dim("→")
                    );
                    println!("          cargo build -p qqbot-core --release");
                    println!("          ./target/release/qqbot restart");
                    global_hints.push(
                        "The running qqbot-core is outdated. Run `cargo build -p qqbot-core --release && ./target/release/qqbot restart`."
                            .to_string(),
                    );
                } else {
                    println!(
                        "{} Could not query runtime group status: {e}",
                        term.warn("[warn]")
                    );
                    global_hints.push(format!(
                        "Could not query runtime group status from qqbot-core: {e}"
                    ));
                }
                allowed_groups
                    .iter()
                    .map(|g| GroupRuntimeStatus {
                        group_id: *g,
                        brain_ready: false,
                        tool_count: 0,
                        tools: Vec::new(),
                    })
                    .collect()
            }
        };

        // Health check can add membership info. Run it once if infra looks ready.
        let health_report = if global_checks.iter().all(|c| c.state != CheckState::Fail) {
            match config.as_ref() {
                Ok(cfg) => {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        health::check(data_dir, cfg, false),
                    )
                    .await
                    {
                        Ok(Ok(report)) => Some(report),
                        Ok(Err(e)) => {
                            println!("{} Health check failed: {e}", term.warn("[warn]"));
                            global_hints.push(format!(
                                "Health-check error: {e}. Stop qqbot-core before running `qqbot health` if NapCat rejects concurrent clients."
                            ));
                            None
                        }
                        Err(_) => {
                            println!("{} Health check timed out", term.warn("[warn]"));
                            global_hints.push(
                                "Health check timed out. Wait a few seconds or run `qqbot restart`."
                                    .to_string(),
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "{} Could not read config for health check: {e}",
                        term.warn("[warn]")
                    );
                    global_hints.push(
                        "Could not read qqbot-core config. Run `qqbot init` or check the data directory."
                            .to_string(),
                    );
                    None
                }
            }
        } else {
            None
        };

        for status in &group_statuses {
            let membership = health_report.as_ref().and_then(|r| {
                r.group_membership
                    .iter()
                    .find(|g| g.group_id == status.group_id)
            });

            let state_symbol = if status.brain_ready {
                term.ok("[ok]")
            } else {
                term.fail("[fail]")
            };
            print!("{state_symbol} Group {}", status.group_id);
            if let Some(m) = membership {
                if m.member {
                    print!(" — {}", term.ok("member"));
                } else {
                    print!(" — {}", term.fail("NOT a member"));
                }
            }
            println!();

            if status.tools.is_empty() {
                println!(
                    "       {} tools: {}",
                    term.dim("→"),
                    term.dim("none loaded")
                );
                if status.brain_ready {
                    global_hints.push(format!(
                        "Group {} has a Brain but no plugin tools loaded. Install a plugin and reload.",
                        status.group_id
                    ));
                } else {
                    global_hints.push(format!(
                        "Group {} Brain is not ready. Check `qqbot logs core -n 50`.",
                        status.group_id
                    ));
                }
            } else {
                let by_source = group_tools_by_source(&status.tools);
                println!("       {} tools ({}):", term.dim("→"), status.tool_count);
                for (source, names) in by_source {
                    println!(
                        "           {} {}",
                        term.dim(&format!("[{}]", source)),
                        names.join(", ")
                    );
                }
            }

            if let Some(m) = membership {
                if !m.member {
                    global_hints.push(format!(
                        "Add the bot account to group {} so it can receive and send messages.",
                        status.group_id
                    ));
                }
            }
        }

        // Print a single health line if the overall report is happy.
        if let Some(report) = health_report.as_ref() {
            if report.online && report.bot_user_id.is_some() {
                println!();
                println!("{} Bot is online", term.ok("[ok]"));
                if let (Some(uid), Some(nick)) = (report.bot_user_id, report.bot_nickname.as_ref())
                {
                    println!(
                        "       {} Logged in as {} ({})",
                        term.dim("→"),
                        term.bold(nick),
                        uid
                    );
                }
            } else if !report.online {
                println!();
                println!("{} QQ account is not online", term.fail("[fail]"));
                global_hints.push(
                    "The bot account is offline. Open the SnowLuma WebUI (http://localhost:5099) to log in or scan the QR code.".to_string(),
                );
            } else if report.bot_user_id.is_none() {
                println!();
                println!("{} Could not determine bot user id", term.fail("[fail]"));
                global_hints.push(
                    "OneBot did not report login info. Check `qqbot logs core -n 50` and `qqbot doctor`.".to_string(),
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Section 3: Troubleshooting & Static Info
    // ------------------------------------------------------------------
    println!();
    println!("{}", term.section("Troubleshooting"));

    // Remove duplicate hints while preserving order.
    let mut seen = std::collections::HashSet::new();
    let hints: Vec<String> = global_hints
        .into_iter()
        .filter(|h| seen.insert(h.clone()))
        .collect();

    if hints.is_empty() {
        println!("{} No issues detected.", term.ok("✓"));
    } else {
        for hint in &hints {
            println!("  {} {}", term.warn("•"), hint);
        }
        println!();
        println!("{} For a full diagnosis run `qqbot doctor`.", term.dim("→"));
    }

    println!();
    println!(
        "{} {}",
        term.dim("Data directory:"),
        base_dir(data_dir).display()
    );
    println!(
        "{} {}",
        term.dim("WebUI:"),
        hyperlink("http://localhost:5099", "http://localhost:5099")
    );
    println!(
        "{} {} (password: {})",
        term.dim("noVNC:"),
        hyperlink("http://localhost:6081", "http://localhost:6081"),
        term.dim("vncpasswd")
    );

    Ok(())
}

fn print_check(check: &Check, hints: &mut Vec<String>) {
    let term = TerminalStyle::new();
    let symbol = match check.state {
        CheckState::Ok => term.ok("[ok]"),
        CheckState::Warn => term.warn("[warn]"),
        CheckState::Fail => term.fail("[fail]"),
    };
    let suffix = check
        .detail
        .as_ref()
        .map(|d| format!(" ({})", term.dim(d)))
        .unwrap_or_default();
    println!("{symbol} {}{suffix}", term.bold(&check.label));
    if let Some(hint) = &check.hint {
        hints.push(hint.clone());
    }
}

/// Group tools by their source label, sorting names within each source.
fn group_tools_by_source(tools: &[ToolRuntimeInfo]) -> Vec<(String, Vec<String>)> {
    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for tool in tools {
        map.entry(tool.source.clone())
            .or_default()
            .push(tool.name.clone());
    }
    map.into_iter()
        .map(|(source, mut names)| {
            names.sort();
            (source, names)
        })
        .collect()
}

fn hyperlink(url: &str, text: &str) -> String {
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

/// Check whether the running qqbot-core binary was built with a different
/// Cargo profile than the current qqbot CLI. This catches the common dev
/// mistake of running `cargo run --bin qqbot -- status` while the daemon is
/// still using a release (or stale debug) core binary.
#[cfg(unix)]
fn core_profile_mismatch(data_dir: &Path) -> Option<String> {
    let run = crate::service::run_dir(data_dir);
    let pid_file = run.join("qqbot-core.pid");
    let pid = std::fs::read_to_string(&pid_file)
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()?;

    let exe = std::env::current_exe().ok()?;
    let my_profile = if exe.to_string_lossy().contains("/debug/") {
        "debug"
    } else {
        "release"
    };

    let core_exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let core_profile = if core_exe.to_string_lossy().contains("/debug/") {
        "debug"
    } else {
        "release"
    };

    if my_profile != core_profile {
        Some(format!(
            "CLI is {my_profile}, running core is {core_profile} ({})",
            core_exe.display()
        ))
    } else {
        None
    }
}

#[cfg(not(unix))]
fn core_profile_mismatch(_data_dir: &Path) -> Option<String> {
    None
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

/// Minimal terminal styling helper.
///
/// Produces ANSI escape sequences when stdout is a terminal; otherwise returns
/// the plain text so pipes and files stay readable.
struct TerminalStyle {
    color: bool,
}

impl TerminalStyle {
    fn new() -> Self {
        Self {
            color: std::io::stdout().is_terminal(),
        }
    }

    fn wrap(&self, text: &str, code: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn bold(&self, text: &str) -> String {
        self.wrap(text, "1")
    }

    fn dim(&self, text: &str) -> String {
        self.wrap(text, "2")
    }

    fn ok(&self, text: &str) -> String {
        self.wrap(text, "1;32")
    }

    fn warn(&self, text: &str) -> String {
        self.wrap(text, "1;33")
    }

    fn fail(&self, text: &str) -> String {
        self.wrap(text, "1;31")
    }

    fn info(&self, text: &str) -> String {
        self.wrap(text, "1;34")
    }

    fn section(&self, text: &str) -> String {
        let inner = format!("== {text} ==");
        if self.color {
            format!("\x1b[1;97m{inner}\x1b[0m")
        } else {
            inner
        }
    }
}
