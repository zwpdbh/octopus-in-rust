use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// Deployment configuration read from `<data-dir>/deploy.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct DeployConfig {
    #[serde(default)]
    pub aliyun: AliyunConfig,
    #[serde(default)]
    pub remote: RemoteConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AliyunConfig {
    pub region: String,
    pub zone: String,
    pub instance_type: String,
    pub image_id: String,
    #[serde(default = "default_vpc_cidr")]
    pub vpc_cidr: String,
    #[serde(default = "default_vswitch_cidr")]
    pub vswitch_cidr: String,
    pub key_pair_name: String,
    #[serde(default = "default_allowed_ssh_cidr")]
    pub allowed_ssh_cidr: String,
    #[serde(default = "default_allowed_service_cidr")]
    pub allowed_service_cidr: String,
    #[serde(default)]
    pub aliyun_profile: Option<String>,
    #[serde(default = "default_name")]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_remote_user")]
    pub user: String,
    #[serde(default = "default_install_dir")]
    pub install_dir: String,
    #[serde(default = "default_ssh_private_key")]
    pub ssh_private_key: String,
}

impl DeployConfig {
    /// Load `deploy.toml` from the given data directory.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("deploy.toml");
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read deployment config: {}", path.display()))?;
        let mut config: DeployConfig = toml::from_str(&contents)
            .with_context(|| format!("failed to parse deployment config: {}", path.display()))?;
        config.remote.ssh_private_key = expand_tilde(&config.remote.ssh_private_key)?;
        Ok(config)
    }

    /// Path to the local SSH private key.
    pub fn ssh_key_path(&self) -> PathBuf {
        PathBuf::from(&self.remote.ssh_private_key)
    }
}

impl Default for AliyunConfig {
    fn default() -> Self {
        Self {
            region: String::new(),
            zone: String::new(),
            instance_type: String::new(),
            image_id: String::new(),
            vpc_cidr: default_vpc_cidr(),
            vswitch_cidr: default_vswitch_cidr(),
            key_pair_name: String::new(),
            allowed_ssh_cidr: default_allowed_ssh_cidr(),
            allowed_service_cidr: default_allowed_service_cidr(),
            aliyun_profile: None,
            name: default_name(),
        }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            user: default_remote_user(),
            install_dir: default_install_dir(),
            ssh_private_key: default_ssh_private_key(),
        }
    }
}

fn default_vpc_cidr() -> String {
    "192.168.0.0/16".to_string()
}

fn default_vswitch_cidr() -> String {
    "192.168.0.0/24".to_string()
}

fn default_allowed_ssh_cidr() -> String {
    "0.0.0.0/0".to_string()
}

fn default_allowed_service_cidr() -> String {
    "0.0.0.0/0".to_string()
}

fn default_name() -> String {
    "octopus-qqbot".to_string()
}

fn default_remote_user() -> String {
    "qqbot".to_string()
}

fn default_install_dir() -> String {
    "/opt/qqbot".to_string()
}

fn default_ssh_private_key() -> String {
    "~/.ssh/id_rsa".to_string()
}

fn expand_tilde(path: &str) -> Result<String> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .context("could not determine home directory to expand '~'")?;
        Ok(format!("{}/{}", home, rest))
    } else {
        Ok(path.to_string())
    }
}
