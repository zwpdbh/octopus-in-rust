use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::aliyun::{save_private_key, AliyunCli, InstanceInfo};
use super::config::DeployConfig;

/// Ensure all AliCloud resources exist and return the running instance info.
pub fn ensure_resources(cli: &AliyunCli, config: &DeployConfig) -> Result<InstanceInfo> {
    let aliyun = &config.aliyun;
    let name = aliyun.name.clone();

    println!("Provisioning AliCloud resources for '{}'...", name);

    let vpc_id = ensure_vpc(cli, config)?;
    println!("  VPC: {}", vpc_id);

    let vswitch_id = ensure_vswitch(cli, config, &vpc_id)?;
    println!("  VSwitch: {}", vswitch_id);

    let sg_id = ensure_security_group(cli, config, &vpc_id)?;
    println!("  SecurityGroup: {}", sg_id);
    ensure_service_ports(cli, config, &sg_id)?;

    ensure_key_pair(cli, config)?;
    println!("  KeyPair: {}", aliyun.key_pair_name);

    let info = ensure_instance(cli, config, &vswitch_id, &sg_id)?;
    println!(
        "  Instance: {} ({}) at {}",
        info.instance_id,
        info.status,
        info.public_ip.as_deref().unwrap_or("<no public ip>")
    );

    Ok(info)
}

fn ensure_vpc(cli: &AliyunCli, config: &DeployConfig) -> Result<String> {
    let aliyun = &config.aliyun;
    if let Some(id) = cli.find_vpc(&aliyun.region, &aliyun.name)? {
        return Ok(id);
    }
    let id = cli.create_vpc(&aliyun.region, &aliyun.vpc_cidr, &aliyun.name)?;
    wait_vpc_available(cli, config, &id)?;
    Ok(id)
}

fn wait_vpc_available(cli: &AliyunCli, config: &DeployConfig, vpc_id: &str) -> Result<()> {
    let region = &config.aliyun.region;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let res = cli.run(
            "vpc",
            "DescribeVpcAttribute",
            &[
                ("RegionId", region.to_string()),
                ("VpcId", vpc_id.to_string()),
            ],
        )?;
        if res["Status"].as_str() == Some("Available") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for VPC {vpc_id} to become Available");
        }
        thread::sleep(Duration::from_secs(3));
    }
}

fn ensure_vswitch(cli: &AliyunCli, config: &DeployConfig, vpc_id: &str) -> Result<String> {
    let aliyun = &config.aliyun;
    if let Some(id) = cli.find_vswitch(&aliyun.region, vpc_id, &aliyun.zone, &aliyun.name)? {
        return Ok(id);
    }
    cli.create_vswitch(
        &aliyun.region,
        vpc_id,
        &aliyun.zone,
        &aliyun.vswitch_cidr,
        &aliyun.name,
    )
}

fn ensure_security_group(cli: &AliyunCli, config: &DeployConfig, vpc_id: &str) -> Result<String> {
    let aliyun = &config.aliyun;
    if let Some(id) = cli.find_security_group(&aliyun.region, vpc_id, &aliyun.name)? {
        return Ok(id);
    }
    let id = cli.create_security_group(&aliyun.region, vpc_id, &aliyun.name)?;
    // Allow SSH from the configured CIDR.
    cli.authorize_ingress(
        &aliyun.region,
        &id,
        "tcp",
        "22/22",
        &aliyun.allowed_ssh_cidr,
    )?;
    Ok(id)
}

/// Ensure SnowLuma management ports are reachable from the configured CIDR.
fn ensure_service_ports(cli: &AliyunCli, config: &DeployConfig, sg_id: &str) -> Result<()> {
    let aliyun = &config.aliyun;
    let rules = cli.list_ingress_rules(&aliyun.region, sg_id)?;
    let needed = [
        ("tcp", "5099/5099"),
        ("tcp", "6081/6081"),
        ("tcp", "5900/5900"),
    ];
    for (protocol, port_range) in needed {
        let already_open = rules.iter().any(|(p, r, c)| {
            p.eq_ignore_ascii_case(protocol) && r == port_range && c == &aliyun.allowed_service_cidr
        });
        if already_open {
            continue;
        }
        cli.authorize_ingress(
            &aliyun.region,
            sg_id,
            protocol,
            port_range,
            &aliyun.allowed_service_cidr,
        )?;
        println!(
            "  Opened {protocol} {port_range} from {}",
            aliyun.allowed_service_cidr
        );
    }
    Ok(())
}

fn ensure_key_pair(cli: &AliyunCli, config: &DeployConfig) -> Result<()> {
    let aliyun = &config.aliyun;
    if cli.key_pair_exists(&aliyun.region, &aliyun.key_pair_name)? {
        return Ok(());
    }
    let pem = cli.create_key_pair(&aliyun.region, &aliyun.key_pair_name)?;
    let key_path = config.ssh_key_path();
    save_private_key(&key_path, &pem)?;
    println!(
        "    New key pair saved to {} (permissions 600)",
        key_path.display()
    );
    Ok(())
}

fn ensure_instance(
    cli: &AliyunCli,
    config: &DeployConfig,
    vswitch_id: &str,
    sg_id: &str,
) -> Result<InstanceInfo> {
    let aliyun = &config.aliyun;
    if let Some(info) = cli.find_instance(&aliyun.region, &aliyun.name)? {
        match info.status.as_str() {
            "Running" => return Ok(info),
            "Stopped" => {
                println!("    Existing instance is Stopped; starting it...");
                cli.start_instance(&aliyun.region, &info.instance_id)?;
            }
            _ => {
                println!(
                    "    Existing instance is in state {}; waiting for Running...",
                    info.status
                );
            }
        }
        return cli.wait_for_running(&aliyun.region, &aliyun.name, Duration::from_secs(300));
    }

    let instance_id = cli.run_instance(
        &aliyun.region,
        &aliyun.zone,
        &aliyun.image_id,
        &aliyun.instance_type,
        vswitch_id,
        sg_id,
        &aliyun.key_pair_name,
        &aliyun.name,
    )?;
    println!("    Created instance {}", instance_id);
    cli.wait_for_running(&aliyun.region, &aliyun.name, Duration::from_secs(300))
}

/// Delete the ECS instance managed by this configuration.
pub fn destroy_instance(cli: &AliyunCli, config: &DeployConfig) -> Result<()> {
    let aliyun = &config.aliyun;
    let info = cli
        .find_instance(&aliyun.region, &aliyun.name)?
        .context("no instance found to destroy")?;
    println!(
        "Deleting instance {} ({})...",
        info.instance_id,
        info.public_ip.as_deref().unwrap_or("<no ip>")
    );
    cli.delete_instance(&aliyun.region, &info.instance_id)?;
    Ok(())
}
