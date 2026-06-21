use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Minimal SSH session wrapping the local `ssh`/`scp` binaries.
#[derive(Debug, Clone)]
pub struct SshSession {
    pub user: String,
    pub host: String,
    pub key: String,
}

impl SshSession {
    fn ssh_base(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg("-i").arg(&self.key);
        cmd.arg("-o").arg("StrictHostKeyChecking=no");
        cmd.arg("-o").arg("UserKnownHostsFile=/dev/null");
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg(format!("{}@{}", self.user, self.host));
        cmd
    }

    fn scp_base(&self) -> Command {
        let mut cmd = Command::new("scp");
        cmd.arg("-i").arg(&self.key);
        cmd.arg("-o").arg("StrictHostKeyChecking=no");
        cmd.arg("-o").arg("UserKnownHostsFile=/dev/null");
        cmd
    }

    /// Run a remote command and return its stdout.
    pub fn run(&self, command: &str) -> Result<String> {
        let mut cmd = self.ssh_base();
        cmd.arg(command);
        let output = cmd
            .output()
            .with_context(|| format!("failed to ssh to {}@{}", self.user, self.host))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("remote command failed ({}): {stderr}", output.status);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run a remote command, streaming stdout/stderr to this process.
    pub fn run_stream(&self, command: &str) -> Result<()> {
        let mut cmd = self.ssh_base();
        cmd.arg(command);
        let status = cmd
            .status()
            .with_context(|| format!("failed to ssh to {}@{}", self.user, self.host))?;
        if !status.success() {
            bail!("remote command failed ({status})");
        }
        Ok(())
    }

    /// Upload a single file.
    pub fn upload<P: AsRef<Path>, Q: AsRef<Path>>(&self, local: P, remote: Q) -> Result<()> {
        let local = local.as_ref();
        let remote = remote.as_ref();
        let dest = format!("{}@{}:{}", self.user, self.host, remote.display());
        let mut cmd = self.scp_base();
        cmd.arg(local).arg(dest);
        let status = cmd.status().with_context(|| {
            format!(
                "failed to scp {} to {}@{}",
                local.display(),
                self.user,
                self.host
            )
        })?;
        if !status.success() {
            bail!("scp upload failed ({status})");
        }
        Ok(())
    }

    /// Recursively upload a directory.
    pub fn upload_dir<P: AsRef<Path>, Q: AsRef<Path>>(&self, local: P, remote: Q) -> Result<()> {
        let local = local.as_ref();
        let remote = remote.as_ref();
        let dest = format!("{}@{}:{}", self.user, self.host, remote.display());
        let mut cmd = self.scp_base();
        cmd.arg("-r").arg(local).arg(dest);
        let status = cmd.status().with_context(|| {
            format!(
                "failed to scp -r {} to {}@{}",
                local.display(),
                self.user,
                self.host
            )
        })?;
        if !status.success() {
            bail!("scp upload failed ({status})");
        }
        Ok(())
    }
}

/// Prefix a command with `sudo -n` unless running as root.
pub fn maybe_sudo(user: &str, command: &str) -> String {
    if user == "root" {
        command.to_string()
    } else {
        format!("sudo -n {command}")
    }
}
