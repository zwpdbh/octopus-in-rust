use crate::paths;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, warn};

pub const SNOWLUMA_IMAGE: &str = "motricseven7/snowluma:latest";
pub const SNOWLUMA_CONTAINER: &str = "snowluma";

pub fn base_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

pub fn logs_dir(data_dir: &Path) -> PathBuf {
    base_dir(data_dir).join("logs")
}

pub fn run_dir(data_dir: &Path) -> PathBuf {
    base_dir(data_dir).join("run")
}

pub fn snowluma_data_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("snowluma-data")
}

/// Run the daemon service loop: manage SnowLuma Docker and qqbot-core.
/// This function blocks until SIGTERM/SIGINT is received and restarts
/// failed components automatically.
pub async fn run(data_dir: &Path) -> Result<()> {
    let base = base_dir(data_dir);
    let logs = logs_dir(data_dir);
    std::fs::create_dir_all(&logs)?;

    let supervisor_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("supervisor.log"))
        .context("failed to open supervisor log")?;
    let supervisor_log_clone = supervisor_log
        .try_clone()
        .context("failed to clone supervisor log fd")?;
    let make_writer = move || {
        supervisor_log_clone
            .try_clone()
            .expect("failed to clone supervisor log fd")
    };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if let Err(e) = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(make_writer)
        .with_ansi(false)
        .try_init()
    {
        use std::io::Write;
        let mut fallback = supervisor_log
            .try_clone()
            .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
        let _ = writeln!(fallback, "failed to initialize tracing subscriber: {e}");
    }

    // Ensure SnowLuma is running.
    if let Err(e) = start_snowluma(&base).await {
        error!(error = %e, "failed to start SnowLuma container");
        return Err(e);
    }

    // Wait for the OneBot WebSocket port to be reachable before starting
    // qqbot-core, so the core does not spin-crash while SnowLuma is still
    // booting.
    if let Err(e) = wait_for_port("127.0.0.1", 3001, 60).await {
        warn!(error = %e, "SnowLuma WebSocket port not reachable yet; qqbot-core will retry connection");
    }

    // Start qqbot-core. Resolve paths from the project root so the daemon
    // works regardless of the current working directory.
    let core_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("core.log"))
        .context("failed to open core log")?;
    let run = run_dir(data_dir);

    // Infer Cargo profile from the running qqbot binary path so `cargo run`
    // picks the matching debug qqbot-core binary.
    let exe = std::env::current_exe().unwrap_or_default();
    let profile = if exe.to_string_lossy().contains("/debug/") {
        "debug"
    } else {
        "release"
    };
    let core_binary = paths::project_root().join(format!("target/{profile}/qqbot-core"));
    if !core_binary.exists() {
        anyhow::bail!(
            "qqbot-core binary not found: {}. Build with `cargo build -p qqbot-core`.",
            core_binary.display()
        );
    }

    let core_config = data_dir.join("config.toml");
    info!(binary = %core_binary.display(), config = %core_config.display(), "starting qqbot-core");

    let core = spawn_core(&core_binary, &core_config, &run, &core_log).await?;
    let mut core_handle = CoreHandle::new(core, core_binary, core_config, core_log, run);

    // Watchdog: restart SnowLuma container and core if either fails.
    let mut container_check = tokio::time::interval(tokio::time::Duration::from_secs(5));
    container_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut core_check = tokio::time::interval(tokio::time::Duration::from_secs(5));
    core_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Wait for shutdown signal.
    let mut sigterm = signal(SignalKind::terminate()).context("failed to bind SIGTERM")?;
    let mut sigint = signal(SignalKind::interrupt()).context("failed to bind SIGINT")?;

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("SIGTERM received, shutting down");
                break;
            }
            _ = sigint.recv() => {
                info!("SIGINT received, shutting down");
                break;
            }
            _ = container_check.tick() => {
                match container_running().await {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!("SnowLuma container is not running; restarting");
                        core_handle.stop().await;
                        if let Err(e) = start_snowluma(&base).await {
                            error!(error = %e, "failed to restart SnowLuma container");
                        } else if let Err(e) = core_handle.restart().await {
                            error!(error = %e, "failed to restart qqbot-core after SnowLuma restart");
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "failed to query SnowLuma container status");
                    }
                }
            }
            _ = core_check.tick() => {
                if core_handle.exited().await {
                    warn!("qqbot-core exited; restarting");
                    if let Err(e) = core_handle.restart().await {
                        error!(error = %e, "failed to restart qqbot-core");
                    }
                }
            }
        }
    }

    core_handle.stop().await;

    if let Err(e) = stop_snowluma().await {
        warn!(error = %e, "failed to stop SnowLuma container");
    }

    let pid_file = run_dir(data_dir).join("qqbot.pid");
    if pid_file.exists() {
        let _ = std::fs::remove_file(&pid_file);
        info!("removed daemon pid file");
    }

    Ok(())
}

async fn spawn_core(
    binary: &Path,
    config: &Path,
    run_dir: &Path,
    log: &std::fs::File,
) -> Result<Child> {
    let mut cmd = Command::new(binary);
    cmd.arg(config)
        .env("RUST_LOG", "info")
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log.try_clone()?));
    let child = cmd.spawn().context("failed to spawn qqbot-core")?;

    // Track the core pid so `qqbot plugin reload` can signal it.
    if let Some(pid) = child.id() {
        let pid_file = run_dir.join("qqbot-core.pid");
        let _ = std::fs::write(&pid_file, pid.to_string());
    }

    Ok(child)
}

struct CoreHandle {
    child: Option<Child>,
    binary: PathBuf,
    config: PathBuf,
    log: std::fs::File,
    run_dir: PathBuf,
}

impl CoreHandle {
    fn new(
        child: Child,
        binary: PathBuf,
        config: PathBuf,
        log: std::fs::File,
        run_dir: PathBuf,
    ) -> Self {
        Self {
            child: Some(child),
            binary,
            config,
            log,
            run_dir,
        }
    }

    async fn restart(&mut self) -> Result<()> {
        self.stop().await;
        if let Err(e) = wait_for_port("127.0.0.1", 3001, 60).await {
            warn!(error = %e, "SnowLuma WebSocket port not reachable yet; qqbot-core will retry");
        }
        let child = spawn_core(&self.binary, &self.config, &self.run_dir, &self.log).await?;
        self.child = Some(child);
        info!("qqbot-core restarted");
        Ok(())
    }

    async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
            info!("qqbot-core stopped");
        }
        let pid_file = self.run_dir.join("qqbot-core.pid");
        let _ = std::fs::remove_file(&pid_file);
    }

    async fn exited(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(_) => true,
            }
        } else {
            true
        }
    }
}

pub(crate) async fn start_snowluma(base_dir: &Path) -> Result<()> {
    let sd = snowluma_data_dir(base_dir);
    std::fs::create_dir_all(sd.join("config"))?;
    std::fs::create_dir_all(sd.join(".config"))?;
    std::fs::create_dir_all(sd.join(".local/share"))?;

    // Ensure default OneBot config exists.
    let onebot_config = sd.join("config").join("onebot.json");
    if !onebot_config.exists() {
        let default = default_snowluma_onebot_config();
        std::fs::write(&onebot_config, default)?;
    }

    // Check if container already exists/running.
    let output = Command::new("docker")
        .args(["ps", "-q", "-f", &format!("name={SNOWLUMA_CONTAINER}")])
        .output()
        .await
        .context("failed to query docker ps")?;
    let running = !output.stdout.is_empty();

    if running {
        info!("SnowLuma container already running");
        return Ok(());
    }

    // Remove stale container if present.
    let _ = Command::new("docker")
        .args(["rm", "-f", SNOWLUMA_CONTAINER])
        .output()
        .await;

    let abs_base = std::fs::canonicalize(base_dir)?;
    let abs_sd = abs_base.join("snowluma-data");

    info!(image = SNOWLUMA_IMAGE, "starting SnowLuma container");

    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "-d",
        "--name",
        SNOWLUMA_CONTAINER,
        "--hostname",
        "snowluma",
        "--mac-address",
        "02:42:ac:11:00:99",
        "--restart",
        "unless-stopped",
        "--shm-size=1g",
        "--cap-add=SYS_PTRACE",
        "--security-opt",
        "seccomp=unconfined",
        "-e",
        "VNC_PASSWD=vncpasswd",
        "-e",
        "SNOWLUMA_WEBUI_PORT=5099",
        "-e",
        "SNOWLUMA_HOOK_AUTOLOAD=1",
        "-p",
        "5900:5900",
        "-p",
        "6081:6081",
        "-p",
        "5099:5099",
        "-p",
        "3000:3000",
        "-p",
        "3001:3001",
        "-v",
        &format!("{}:/app/snowluma-data", abs_sd.display()),
        "-v",
        &format!("{}:/app/.config", abs_sd.join(".config").display()),
        "-v",
        &format!(
            "{}:/app/.local/share",
            abs_sd.join(".local/share").display()
        ),
        SNOWLUMA_IMAGE,
    ]);

    let output = cmd.output().await.context("failed to run docker")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to start SnowLuma container: {stderr}");
    }

    info!("SnowLuma container started");
    Ok(())
}

pub(crate) async fn stop_snowluma() -> Result<()> {
    let output = Command::new("docker")
        .args(["stop", "-t", "10", SNOWLUMA_CONTAINER])
        .output()
        .await
        .context("failed to stop SnowLuma container")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to stop SnowLuma container: {stderr}");
    }
    info!("SnowLuma container stopped");
    Ok(())
}

pub(crate) async fn container_running() -> Result<bool> {
    let output = Command::new("docker")
        .args(["ps", "-q", "-f", &format!("name={SNOWLUMA_CONTAINER}")])
        .output()
        .await
        .context("failed to query docker ps")?;
    Ok(!output.stdout.is_empty())
}

pub(crate) async fn wait_for_port(host: &str, port: u16, timeout_secs: u64) -> Result<()> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    anyhow::bail!("timed out waiting for {host}:{port}")
}

/// Try to read the SnowLuma WebUI initial password from the container logs.
/// SnowLuma prints this only once, on the first start of a fresh data volume.
/// We poll for a few seconds because Docker may need a moment to flush the line.
pub(crate) async fn extract_snowluma_webui_password() -> Option<String> {
    for _ in 0..10 {
        let output = match Command::new("sh")
            .args([
                "-c",
                &format!(
                    "docker logs {SNOWLUMA_CONTAINER} 2>&1 | grep -E 'initial credentials|临时密码' | tail -n 1"
                ),
            ])
            .output()
            .await
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        };

        let line = output.trim();
        if !line.is_empty() {
            if let Some(idx) = line.find("password=") {
                let rest = &line[idx + "password=".len()..];
                return rest.split_whitespace().next().map(|s| s.to_string());
            }
            if let Some(idx) = line.find("临时密码:") {
                let rest = &line[idx + "临时密码:".len()..];
                return rest.split_whitespace().next().map(|s| s.to_string());
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    None
}

pub fn default_snowluma_onebot_config() -> String {
    r#"{
  "networks": {
    "httpServers": [],
    "httpClients": [],
    "wsServers": [
      {
        "name": "qqbot-ws",
        "enabled": true,
        "host": "0.0.0.0",
        "port": 3001,
        "path": "/",
        "role": "Universal",
        "accessToken": "",
        "messageFormat": "array",
        "reportSelfMessage": true
      }
    ],
    "wsClients": []
  },
  "musicSignUrl": "",
  "statusCommand": {
    "enabled": true,
    "swallow": false,
    "cooldownSeconds": 5
  }
}
"#
    .to_string()
}
