use std::io::Write;

use anyhow::{bail, Context, Result};

use crate::project;

use super::aliyun::{AliyunCli, InstanceInfo};
use super::config::DeployConfig;
use super::provision;
use super::remote;

/// Full deploy: provision AliCloud resources and install/upgrade the service.
pub fn run() -> Result<()> {
    let config = load_config()?;
    let cli = AliyunCli::new(
        config.aliyun.aliyun_profile.clone(),
        config.aliyun.region.clone(),
    );

    let instance = provision::ensure_resources(&cli, &config)?;
    let ip = instance
        .public_ip
        .as_deref()
        .context("instance has no public IP yet")?;
    println!("\nInstance ready at {ip}");

    remote::install(&config, &instance)?;
    println!("\nDeploy complete.");
    Ok(())
}

/// Run a read-only `qqbot` subcommand on the remote host.
pub fn remote_cmd(subcommand: &str) -> Result<()> {
    let (config, instance) = load_config_and_instance()?;
    let output = remote::run_qqbot(&config, &instance, subcommand)?;
    print!("{output}");
    Ok(())
}

/// Show remote logs. Extra args are forwarded to `qqbot logs`.
pub fn remote_logs(rest: &[String]) -> Result<()> {
    let (config, instance) = load_config_and_instance()?;
    let mut cmd = vec!["logs".to_string()];
    if rest.is_empty() {
        cmd.extend(["core".to_string(), "-n".to_string(), "50".to_string()]);
    } else {
        cmd.extend(rest.iter().cloned());
    }
    let output = remote::run_qqbot(&config, &instance, &cmd.join(" "))?;
    print!("{output}");
    Ok(())
}

/// Control the remote systemd service (`start`, `stop`, `restart`).
pub fn remote_service_cmd(action: &str) -> Result<()> {
    let (config, instance) = load_config_and_instance()?;
    match action {
        "start" => remote::start_service(&config, &instance)?,
        "stop" => remote::stop_service(&config, &instance)?,
        "restart" => remote::restart_service(&config, &instance)?,
        other => bail!("unknown remote service action: {other}"),
    }
    println!("Remote service {action} complete.");
    Ok(())
}

/// Start the remote ECS instance if it is currently stopped.
pub fn remote_start_instance() -> Result<()> {
    let config = load_config()?;
    let cli = AliyunCli::new(
        config.aliyun.aliyun_profile.clone(),
        config.aliyun.region.clone(),
    );

    let info = cli
        .find_instance(&config.aliyun.region, &config.aliyun.name)?
        .context("no instance found; run `cargo xtask qqbot deploy` first")?;

    match info.status.as_str() {
        "Running" => {
            println!(
                "Instance {} is already Running at {}",
                info.instance_id,
                info.public_ip.as_deref().unwrap_or("<no ip>")
            );
        }
        "Stopped" => {
            println!("Starting instance {} ...", info.instance_id);
            cli.start_instance(&config.aliyun.region, &info.instance_id)?;
            let started = cli.wait_for_running(
                &config.aliyun.region,
                &config.aliyun.name,
                std::time::Duration::from_secs(300),
            )?;
            println!(
                "Instance {} is now Running at {}",
                started.instance_id,
                started.public_ip.as_deref().unwrap_or("<no ip>")
            );
        }
        other => {
            bail!("instance is in unexpected state: {other}");
        }
    }
    Ok(())
}

/// Stop the remote ECS instance to save on compute charges.
pub fn remote_stop_instance() -> Result<()> {
    let config = load_config()?;
    let cli = AliyunCli::new(
        config.aliyun.aliyun_profile.clone(),
        config.aliyun.region.clone(),
    );

    let info = cli
        .find_instance(&config.aliyun.region, &config.aliyun.name)?
        .context("no instance found; run `cargo xtask qqbot deploy` first")?;

    match info.status.as_str() {
        "Stopped" => {
            println!("Instance {} is already Stopped.", info.instance_id);
        }
        "Running" => {
            println!("Stopping instance {} ...", info.instance_id);
            cli.stop_instance(&config.aliyun.region, &info.instance_id)?;
            let stopped = cli.wait_for_stopped(
                &config.aliyun.region,
                &config.aliyun.name,
                std::time::Duration::from_secs(300),
            )?;
            println!("Instance {} is now Stopped.", stopped.instance_id);
        }
        other => {
            bail!("instance is in unexpected state: {other}");
        }
    }
    Ok(())
}

/// Destroy the remote ECS instance after interactive confirmation.
pub fn remote_destroy() -> Result<()> {
    let config = load_config()?;
    let cli = AliyunCli::new(
        config.aliyun.aliyun_profile.clone(),
        config.aliyun.region.clone(),
    );

    let instance = cli
        .find_instance(&config.aliyun.region, &config.aliyun.name)?
        .context("no instance found to destroy")?;

    print!(
        "Are you sure you want to delete instance {} ({})? [y/N] ",
        instance.instance_id,
        instance.public_ip.as_deref().unwrap_or("<no ip>")
    );
    std::io::stdout().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if line.trim().eq_ignore_ascii_case("y") {
        provision::destroy_instance(&cli, &config)?;
        println!("Instance deleted.");
        Ok(())
    } else {
        println!("Cancelled.");
        Ok(())
    }
}

fn load_config() -> Result<DeployConfig> {
    let data_dir = project::data_dir()?;
    DeployConfig::load(&data_dir)
}

fn load_config_and_instance() -> Result<(DeployConfig, InstanceInfo)> {
    let config = load_config()?;
    let cli = AliyunCli::new(
        config.aliyun.aliyun_profile.clone(),
        config.aliyun.region.clone(),
    );
    let instance = cli
        .find_instance(&config.aliyun.region, &config.aliyun.name)?
        .context("no running instance found; run `cargo xtask qqbot deploy` first")?;
    if instance.status != "Running" {
        bail!(
            "instance {} is not Running (status: {})",
            instance.instance_id,
            instance.status
        );
    }
    Ok((config, instance))
}
