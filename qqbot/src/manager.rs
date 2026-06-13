use crate::config::Config;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tracing::info;

pub struct ProcessManager {
    config: Config,
    data_dir: PathBuf,
    napcat_child: Option<Child>,
    core_child: Option<Child>,
}

impl ProcessManager {
    pub fn new(config: Config, data_dir: PathBuf) -> Self {
        Self {
            config,
            data_dir,
            napcat_child: None,
            core_child: None,
        }
    }

    pub async fn setup(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)
            .await
            .context("failed to create data directory")?;
        fs::create_dir_all(self.data_dir.join("plugins"))
            .await
            .context("failed to create plugins dir")?;
        fs::create_dir_all(self.data_dir.join("logs"))
            .await
            .context("failed to create logs dir")?;

        // NapCatQQ keeps its own config/QQ data inside its install directory.
        let napcat_dir = PathBuf::from(&self.config.napcat.dir);
        fs::create_dir_all(napcat_dir.join("config"))
            .await
            .context("failed to create NapCatQQ config dir")?;
        Ok(())
    }

    pub async fn start_napcat(&mut self) -> Result<()> {
        let napcat_dir = PathBuf::from(&self.config.napcat.dir);
        if !napcat_dir.is_dir() {
            anyhow::bail!(
                "NapCatQQ directory not found: {}. Place the NapCatQQ bundle here or use a release tarball that includes it.",
                napcat_dir.display()
            );
        }

        let launcher = napcat_dir.join(&self.config.napcat.launcher);
        if !launcher.exists() {
            anyhow::bail!(
                "NapCatQQ launcher not found: {}. Check napcat.dir and napcat.launcher in config.",
                launcher.display()
            );
        }

        let stdout = Stdio::from(std::fs::File::create(
            self.data_dir.join("logs").join("napcat.stdout.log"),
        )?);
        let stderr = Stdio::from(std::fs::File::create(
            self.data_dir.join("logs").join("napcat.stderr.log"),
        )?);

        info!(dir = %napcat_dir.display(), launcher = %launcher.display(), "starting NapCatQQ");

        let mut cmd = Command::new(&launcher);
        cmd.current_dir(&napcat_dir)
            .env("ACCOUNT", self.config.qq.account.to_string())
            .env("WS_ENABLE", "true")
            .env("NAPCAT_UID", "0")
            .env("NAPCAT_GID", "0")
            .stdout(stdout)
            .stderr(stderr);

        let child = cmd.spawn().context("failed to spawn NapCatQQ process")?;
        self.napcat_child = Some(child);

        // Wait for the OneBot WebSocket port.
        wait_for_port("127.0.0.1", self.config.napcat.ws_port, 60).await?;
        info!(
            port = self.config.napcat.ws_port,
            "NapCatQQ OneBot port is reachable"
        );

        Ok(())
    }

    pub async fn start_core(&mut self) -> Result<()> {
        let core_binary = PathBuf::from(&self.config.core.binary);
        if !core_binary.exists() {
            anyhow::bail!(
                "qqbot-core binary not found: {}. Build or download qqbot-core and place it next to qqbot.",
                core_binary.display()
            );
        }

        let stdout = Stdio::from(std::fs::File::create(
            self.data_dir.join("logs").join("core.stdout.log"),
        )?);
        let stderr = Stdio::from(std::fs::File::create(
            self.data_dir.join("logs").join("core.stderr.log"),
        )?);

        info!(binary = %core_binary.display(), config = %self.config.core.config_path, "starting qqbot-core");

        let mut cmd = Command::new(&core_binary);
        cmd.arg(&self.config.core.config_path)
            .stdout(stdout)
            .stderr(stderr);

        let child = cmd.spawn().context("failed to spawn qqbot-core process")?;
        self.core_child = Some(child);
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.core_child.take() {
            let _ = child.start_kill();
            info!("sent kill signal to qqbot-core");
        }
        if let Some(mut child) = self.napcat_child.take() {
            let _ = child.start_kill();
            info!("sent kill signal to NapCatQQ");
        }
        Ok(())
    }

    pub async fn status(&mut self) -> (bool, bool) {
        let napcat = match &mut self.napcat_child {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        let core = match &mut self.core_child {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        (napcat, core)
    }
}

async fn wait_for_port(host: &str, port: u16, timeout_secs: u64) -> Result<()> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    anyhow::bail!("timed out waiting for {host}:{port}")
}

pub fn napcat_config_path(_data_dir: &Path, napcat_dir: &str, account: i64) -> PathBuf {
    PathBuf::from(napcat_dir)
        .join("config")
        .join(format!("onebot11_{account}.json"))
}

pub fn core_config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.toml")
}
