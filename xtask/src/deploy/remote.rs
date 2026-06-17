use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::aliyun::InstanceInfo;
use super::config::DeployConfig;
use super::ssh::{maybe_sudo, SshSession};

/// Build the release tarball and install/upgrade it on the remote host.
pub fn install(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    let tarball = build_release_tarball()?;
    let ssh = ssh_session(config, instance);

    println!(
        "Connecting to {}@{} ...",
        config.remote.user,
        instance.public_ip.as_deref().unwrap_or("<unknown>")
    );

    // Wait until SSH is reachable (new instances need a few seconds).
    wait_for_ssh(&ssh)?;

    // Bootstrap environment: create user, install Docker, pull SnowLuma image.
    run_setup_script(config, instance, &ssh)?;

    // Ensure install directory exists and binaries can land in bin/.
    let install_dir = &config.remote.install_dir;
    ssh.run_stream(&format!(
        "mkdir -p {install_dir}/bin {install_dir}/data/qqbot-data/plugins {install_dir}/data/logs {install_dir}/data/run"
    ))?;

    // Upload and extract release tarball.
    let remote_tar = format!("{install_dir}/qqbot-linux-x86_64.tar.gz");
    ssh.upload(&tarball, &remote_tar)?;
    ssh.run_stream(&format!(
        "cd {install_dir} && tar -xzf qqbot-linux-x86_64.tar.gz && \
         mv -f qqbot-linux-x86_64/qqbot bin/qqbot && \
         mv -f qqbot-linux-x86_64/qqbot-core bin/qqbot-core && \
         mv -f qqbot-linux-x86_64/plugins/*.wasm data/qqbot-data/plugins/ 2>/dev/null || true && \
         rm -rf qqbot-linux-x86_64 qqbot-linux-x86_64.tar.gz"
    ))?;

    // Sync local configuration, groups, and plugins.
    sync_data_dir(config, instance)?;

    // Set ownership so the service user can write logs/run files.
    let user = &config.remote.user;
    ssh.run_stream(&format!(
        "{chown} -R {user}:{user} {install_dir}/data",
        chown = maybe_sudo(user, "chown")
    ))?;

    // Install and start the systemd unit.
    install_systemd_service(config, instance, &ssh)?;

    // Verify.
    println!("\nRemote service status:");
    let status = run_qqbot(config, instance, "status")?;
    println!("{status}");

    println!("\nRemote doctor:");
    let doctor = run_qqbot(config, instance, "doctor")?;
    println!("{doctor}");

    Ok(())
}

/// Run an arbitrary `qqbot` subcommand on the remote host.
pub fn run_qqbot(
    config: &DeployConfig,
    instance: &InstanceInfo,
    subcommand: &str,
) -> Result<String> {
    let ssh = ssh_session(config, instance);
    let install_dir = &config.remote.install_dir;
    let data_dir = format!("{install_dir}/data/qqbot-data");
    let cmd = format!("{install_dir}/bin/qqbot {subcommand} -d {data_dir}");
    ssh.run(&cmd)
}

/// Run a remote command that requires root (systemctl etc.).
pub fn run_remote_root(
    config: &DeployConfig,
    instance: &InstanceInfo,
    command: &str,
) -> Result<()> {
    let ssh = ssh_session(config, instance);
    let cmd = maybe_sudo(&config.remote.user, command);
    ssh.run_stream(&cmd)
}

/// Reboot the remote systemd service.
pub fn restart_service(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    run_remote_root(config, instance, "systemctl restart qqbot")
}

/// Stop the remote systemd service.
pub fn stop_service(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    run_remote_root(config, instance, "systemctl stop qqbot")
}

/// Start the remote systemd service.
pub fn start_service(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    run_remote_root(config, instance, "systemctl start qqbot")
}

fn build_release_tarball() -> Result<PathBuf> {
    use crate::project;

    let root = project::root();
    let script = root.join("scripts/build-qqbot-release.sh");
    let status = Command::new(&script)
        .current_dir(&root)
        .status()
        .with_context(|| format!("failed to run {}", script.display()))?;
    if !status.success() {
        bail!("{} failed with status {}", script.display(), status);
    }
    Ok(root.join("dist/qqbot-linux-x86_64.tar.gz"))
}

fn ssh_session(config: &DeployConfig, instance: &InstanceInfo) -> SshSession {
    SshSession {
        user: config.remote.user.clone(),
        host: instance
            .public_ip
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string()),
        key: config.remote.ssh_private_key.clone(),
    }
}

fn wait_for_ssh(ssh: &SshSession) -> Result<()> {
    for attempt in 0..30 {
        match ssh.run("echo ok") {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt == 29 {
                    return Err(e).context("SSH did not become available");
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
    Ok(())
}

fn run_setup_script(
    config: &DeployConfig,
    _instance: &InstanceInfo,
    ssh: &SshSession,
) -> Result<()> {
    use crate::project;

    let root = project::root();
    let local_script = root.join("scripts/qqbot-remote-setup.sh");
    let install_dir = &config.remote.install_dir;
    let remote_script = format!("{install_dir}/qqbot-remote-setup.sh");
    ssh.upload(&local_script, &remote_script)?;
    let setup_cmd = maybe_sudo(
        &config.remote.user,
        &format!(
            "bash {remote_script} {user} {install_dir}",
            user = config.remote.user
        ),
    );
    ssh.run_stream(&setup_cmd)?;
    Ok(())
}

fn sync_data_dir(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    use crate::project;

    let ssh = ssh_session(config, instance);
    let local_data = project::data_dir()?.join("qqbot-data");
    let remote_base = format!("{}/data/qqbot-data", config.remote.install_dir);

    ssh.run_stream(&format!(
        "mkdir -p {remote_base}/groups {remote_base}/plugins"
    ))?;

    let config_local = local_data.join("config.toml");
    if config_local.exists() {
        ssh.upload(&config_local, format!("{remote_base}/config.toml"))?;
    }

    let groups_local = local_data.join("groups");
    if groups_local.is_dir() {
        ssh.upload_dir(&groups_local, &remote_base)?;
    }

    let plugins_local = local_data.join("plugins");
    if plugins_local.is_dir() {
        ssh.upload_dir(&plugins_local, &remote_base)?;
    }

    // SnowLuma configuration (onebot.json, webui.json, etc.) is required for
    // `qqbot start` to consider the data directory initialized.
    let local_base = project::data_dir()?;
    let snowluma_config_local = local_base.join("snowluma-data/config");
    if snowluma_config_local.is_dir() {
        let remote_snowluma = format!("{}/data/snowluma-data", config.remote.install_dir);
        ssh.run_stream(&format!("mkdir -p {remote_snowluma}"))?;
        ssh.upload_dir(&snowluma_config_local, &remote_snowluma)?;
    }

    Ok(())
}

fn install_systemd_service(
    config: &DeployConfig,
    _instance: &InstanceInfo,
    ssh: &SshSession,
) -> Result<()> {
    use crate::project;

    let local_unit = project::root().join("scripts/qqbot.service");
    let remote_unit_tmp = "/tmp/qqbot.service".to_string();
    ssh.upload(&local_unit, &remote_unit_tmp)?;

    let user = &config.remote.user;
    let mv_cmd = maybe_sudo(
        user,
        &format!("mv -f {remote_unit_tmp} /etc/systemd/system/qqbot.service"),
    );
    ssh.run_stream(&mv_cmd)?;

    let reload = maybe_sudo(user, "systemctl daemon-reload");
    ssh.run_stream(&reload)?;

    let enable = maybe_sudo(user, "systemctl enable qqbot");
    ssh.run_stream(&enable)?;

    let restart = maybe_sudo(user, "systemctl restart qqbot");
    ssh.run_stream(&restart)?;

    Ok(())
}
