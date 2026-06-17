use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::aliyun::InstanceInfo;
use super::config::DeployConfig;
use super::ssh::{maybe_sudo, SshSession};

/// Build the release tarball and install/upgrade it on the remote host.
pub fn install(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    let tarball = build_release_tarball()?;

    // Fresh ECS instances only have a root user. Bootstrap as root first:
    // create the service user, install Docker, and prepare directories.
    let root_ssh = SshSession {
        user: "root".to_string(),
        host: instance
            .public_ip
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string()),
        key: config.remote.ssh_private_key.clone(),
    };
    println!(
        "Connecting to root@{} for bootstrap ...",
        instance.public_ip.as_deref().unwrap_or("<unknown>")
    );
    wait_for_ssh(&root_ssh)?;
    run_setup_script(config, instance, &root_ssh)?;
    install_service_user_key(config, &root_ssh)?;

    // Subsequent installation steps run as the unprivileged service user.
    let ssh = ssh_session(config, instance);

    println!(
        "Connecting to {}@{} ...",
        config.remote.user,
        instance.public_ip.as_deref().unwrap_or("<unknown>")
    );

    // Wait until SSH is reachable for the service user.
    wait_for_ssh(&ssh)?;

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
    // Upload to /tmp first: the install directory is created by the script itself.
    let remote_script = "/tmp/qqbot-remote-setup.sh";
    ssh.upload(&local_script, remote_script)?;
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

/// Install the SSH public key for the service user so the deployer can log in as that user.
fn install_service_user_key(config: &DeployConfig, ssh: &SshSession) -> Result<()> {
    let user = &config.remote.user;
    let key_path = config.ssh_key_path();

    // Derive the public key from the deployed private key.
    let output = Command::new("ssh-keygen")
        .args(["-y", "-f", key_path.to_string_lossy().as_ref()])
        .output()
        .with_context(|| format!("failed to derive public key from {}", key_path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ssh-keygen failed ({}): {stderr}", output.status);
    }
    let pub_key = String::from_utf8_lossy(&output.stdout);

    // Write it to the service user's authorized_keys as root.
    let auth_keys = format!("/home/{user}/.ssh/authorized_keys");
    ssh.run_stream(&format!(
        "mkdir -p /home/{user}/.ssh && \
         printf '%s' '{pub_key}' > {auth_keys} && \
         chown -R {user}:{user} /home/{user}/.ssh && \
         chmod 700 /home/{user}/.ssh && \
         chmod 600 {auth_keys}",
    ))?;
    Ok(())
}

fn sync_data_dir(config: &DeployConfig, instance: &InstanceInfo) -> Result<()> {
    use crate::project;

    let ssh = ssh_session(config, instance);
    // The .qqbot marker points at the local qqbot-data directory.
    let local_data = project::data_dir()?;
    let remote_base = format!("{}/data/qqbot-data", config.remote.install_dir);

    println!("  Syncing data from {} to {} ...", local_data.display(), remote_base);

    ssh.run_stream(&format!(
        "mkdir -p {remote_base}/groups {remote_base}/plugins"
    ))?;

    let config_local = local_data.join("config.toml");
    if config_local.exists() {
        println!("  Uploading config.toml ...");
        ssh.upload(&config_local, format!("{remote_base}/config.toml"))?;
    } else {
        println!("  No local config.toml found; skipping.");
    }

    let groups_local = local_data.join("groups");
    if groups_local.is_dir() {
        println!("  Uploading groups/ ...");
        ssh.upload_dir(&groups_local, &remote_base)?;
    }

    let plugins_local = local_data.join("plugins");
    if plugins_local.is_dir() {
        println!("  Uploading plugins/ ...");
        ssh.upload_dir(&plugins_local, &remote_base)?;
    }

    // SnowLuma configuration (onebot.json, webui.json, etc.) is required for
    // `qqbot start` to consider the data directory initialized.
    // snowluma-data lives next to qqbot-data under the project data root.
    let local_base = local_data
        .parent()
        .context("qqbot data directory has no parent")?;
    let snowluma_config_local = local_base.join("snowluma-data/config");
    if snowluma_config_local.is_dir() {
        let remote_snowluma = format!("{}/data/snowluma-data", config.remote.install_dir);
        println!("  Uploading snowluma config to {} ...", remote_snowluma);
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
