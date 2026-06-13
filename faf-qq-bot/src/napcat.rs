use crate::config::NapcatConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs;
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// Manages the NapCatQQ child process lifecycle.
pub struct NapcatManager {
    pub(crate) config: NapcatConfig,
    child: Option<Child>,
    pid_file: PathBuf,
}

impl NapcatManager {
    pub fn new(config: NapcatConfig) -> Self {
        let pid_file = PathBuf::from(&config.data_dir).join("napcat.pid");
        Self {
            config,
            child: None,
            pid_file,
        }
    }

    /// Ensure the NapCatQQ data directory exists.
    pub async fn ensure_data_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.config.data_dir)
            .await
            .context("failed to create NapCatQQ data directory")?;
        Ok(())
    }

    /// Start NapCatQQ as a child process.
    pub async fn start(&mut self) -> Result<()> {
        if self.is_running().await {
            info!("NapCatQQ is already running");
            return Ok(());
        }

        self.ensure_data_dir().await?;

        let napcat_dir = PathBuf::from(&self.config.dir);
        if !napcat_dir.is_dir() {
            anyhow::bail!(
                "NapCatQQ directory does not exist: {}. Run `setup` first or install NapCatQQ manually.",
                napcat_dir.display()
            );
        }

        let launcher = napcat_dir.join(&self.config.launch_command);
        if !launcher.exists() {
            anyhow::bail!(
                "NapCatQQ launcher not found: {}. Check napcat.dir and napcat.launch_command in config.toml.",
                launcher.display()
            );
        }

        info!(dir = %napcat_dir.display(), launcher = %launcher.display(), "starting NapCatQQ");

        let mut cmd = Command::new(&launcher);
        cmd.current_dir(&napcat_dir)
            .args(&self.config.launch_args)
            .stdout(Stdio::from(std::fs::File::create(
                PathBuf::from(&self.config.data_dir).join("napcat.stdout.log"),
            )?))
            .stderr(Stdio::from(std::fs::File::create(
                PathBuf::from(&self.config.data_dir).join("napcat.stderr.log"),
            )?));

        let child = cmd.spawn().context("failed to spawn NapCatQQ process")?;
        let pid = child.id().unwrap_or(0);
        self.child = Some(child);

        fs::write(&self.pid_file, pid.to_string())
            .await
            .context("failed to write NapCatQQ pid file")?;

        info!(pid, "NapCatQQ process started");
        Ok(())
    }

    /// Stop the NapCatQQ child process and remove the pid file.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            match child.start_kill() {
                Ok(_) => info!("sent kill signal to NapCatQQ child process"),
                Err(e) => warn!(error = %e, "failed to kill NapCatQQ child process"),
            }
        }

        // Also kill any leftover process recorded in the pid file.
        if let Ok(pid_text) = fs::read_to_string(&self.pid_file).await {
            if let Ok(pid) = pid_text.trim().parse::<u32>() {
                if pid > 0 {
                    #[cfg(unix)]
                    {
                        use std::process::Command as SyncCommand;
                        let _ = SyncCommand::new("kill")
                            .arg("-TERM")
                            .arg(pid.to_string())
                            .output();
                    }
                    #[cfg(windows)]
                    {
                        use std::process::Command as SyncCommand;
                        let _ = SyncCommand::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/F"])
                            .output();
                    }
                }
            }
        }

        let _ = fs::remove_file(&self.pid_file).await;
        info!("NapCatQQ stopped");
        Ok(())
    }

    /// Check whether NapCatQQ appears to be running.
    pub async fn is_running(&mut self) -> bool {
        // If we spawned it in this process, check the child status first.
        if let Some(child) = &mut self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => return true,
                Ok(Some(_)) => {
                    let _ = self.child.take();
                    return false;
                }
                Err(_) => return false,
            }
        }

        // Otherwise check the pid file.
        if let Ok(pid_text) = fs::read_to_string(&self.pid_file).await {
            if let Ok(pid) = pid_text.trim().parse::<u32>() {
                return process_exists(pid);
            }
        }

        false
    }

    /// Wait for the OneBot WebSocket port to become reachable.
    pub async fn wait_for_onebot(&self, ws_url: &str, timeout_secs: u64) -> Result<()> {
        let parsed = url::Url::parse(ws_url).context("invalid OneBot WebSocket URL")?;
        let host = parsed.host_str().unwrap_or("localhost").to_string();
        let port = parsed.port().unwrap_or(3001);

        info!(host, port, "waiting for OneBot WebSocket port");
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

        while tokio::time::Instant::now() < deadline {
            if tokio::net::TcpStream::connect((host.as_str(), port))
                .await
                .is_ok()
            {
                info!("OneBot WebSocket port is reachable");
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        anyhow::bail!("timed out waiting for OneBot WebSocket port {host}:{port}")
    }
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(&format!("/proc/{pid}")).is_dir()
    }
    #[cfg(windows)]
    {
        use std::process::Command as SyncCommand;
        SyncCommand::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}
