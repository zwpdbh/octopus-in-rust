use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// Wrapper around the local `aliyun` CLI.
#[derive(Debug, Clone)]
pub struct AliyunCli {
    profile: Option<String>,
    region: String,
}

impl AliyunCli {
    pub fn new(profile: Option<String>, region: String) -> Self {
        Self { profile, region }
    }

    /// Build a base `aliyun` command with global flags.
    fn base_cmd(&self) -> Command {
        let mut cmd = Command::new("aliyun");
        if let Some(profile) = &self.profile {
            cmd.args(["--profile", profile]);
        }
        cmd.args(["--region", &self.region]);
        // The `aliyun` CLI defaults to JSON output.  Its `--output` flag is
        // reserved for table-format filters (cols=/rows=) and must not be set.
        cmd
    }

    /// Run an AliCloud CLI command and parse the JSON output.
    pub(crate) fn run(
        &self,
        product: &str,
        action: &str,
        args: &[(&str, String)],
    ) -> Result<Value> {
        let mut cmd = self.base_cmd();
        cmd.arg(product).arg(action);
        for (k, v) in args {
            cmd.arg(format!("--{k}")).arg(v);
        }

        let output = cmd
            .output()
            .with_context(|| format!("failed to spawn aliyun {product} {action}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "aliyun {product} {action} failed ({}): {stderr}",
                output.status
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(&stdout).with_context(|| {
            format!("aliyun {product} {action} returned invalid JSON: {stdout}")
        })?;
        Ok(value)
    }

    // ------------------------------------------------------------------
    // VPC
    // ------------------------------------------------------------------

    pub fn find_vpc(&self, region: &str, name: &str) -> Result<Option<String>> {
        let res = self.run(
            "vpc",
            "DescribeVpcs",
            &[
                ("RegionId", region.to_string()),
                ("VpcName", name.to_string()),
            ],
        )?;
        Ok(res["Vpcs"]["Vpc"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v["VpcId"].as_str())
            .map(String::from))
    }

    pub fn create_vpc(&self, region: &str, cidr: &str, name: &str) -> Result<String> {
        let mut args = vec![
            ("RegionId", region.to_string()),
            ("CidrBlock", cidr.to_string()),
            ("VpcName", name.to_string()),
        ];
        add_tags(&mut args, name);
        let res = self.run("vpc", "CreateVpc", &args)?;
        res["VpcId"]
            .as_str()
            .map(String::from)
            .context("CreateVpc response missing VpcId")
    }

    // ------------------------------------------------------------------
    // VSwitch
    // ------------------------------------------------------------------

    pub fn find_vswitch(
        &self,
        region: &str,
        vpc_id: &str,
        zone: &str,
        name: &str,
    ) -> Result<Option<String>> {
        let res = self.run(
            "vpc",
            "DescribeVSwitches",
            &[
                ("RegionId", region.to_string()),
                ("VpcId", vpc_id.to_string()),
                ("ZoneId", zone.to_string()),
                ("VSwitchName", name.to_string()),
            ],
        )?;
        Ok(res["VSwitches"]["VSwitch"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v["VSwitchId"].as_str())
            .map(String::from))
    }

    pub fn create_vswitch(
        &self,
        region: &str,
        vpc_id: &str,
        zone: &str,
        cidr: &str,
        name: &str,
    ) -> Result<String> {
        let mut args = vec![
            ("RegionId", region.to_string()),
            ("VpcId", vpc_id.to_string()),
            ("ZoneId", zone.to_string()),
            ("CidrBlock", cidr.to_string()),
            ("VSwitchName", name.to_string()),
        ];
        add_tags(&mut args, name);
        let res = self.run("vpc", "CreateVSwitch", &args)?;
        res["VSwitchId"]
            .as_str()
            .map(String::from)
            .context("CreateVSwitch response missing VSwitchId")
    }

    // ------------------------------------------------------------------
    // Security Group
    // ------------------------------------------------------------------

    pub fn find_security_group(
        &self,
        region: &str,
        vpc_id: &str,
        name: &str,
    ) -> Result<Option<String>> {
        let res = self.run(
            "ecs",
            "DescribeSecurityGroups",
            &[
                ("RegionId", region.to_string()),
                ("VpcId", vpc_id.to_string()),
                ("SecurityGroupName", name.to_string()),
            ],
        )?;
        Ok(res["SecurityGroups"]["SecurityGroup"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v["SecurityGroupId"].as_str())
            .map(String::from))
    }

    pub fn create_security_group(&self, region: &str, vpc_id: &str, name: &str) -> Result<String> {
        let mut args = vec![
            ("RegionId", region.to_string()),
            ("VpcId", vpc_id.to_string()),
            ("SecurityGroupName", name.to_string()),
        ];
        add_tags(&mut args, name);
        let res = self.run("ecs", "CreateSecurityGroup", &args)?;
        res["SecurityGroupId"]
            .as_str()
            .map(String::from)
            .context("CreateSecurityGroup response missing SecurityGroupId")
    }

    pub fn authorize_ingress(
        &self,
        region: &str,
        sg_id: &str,
        protocol: &str,
        port_range: &str,
        cidr: &str,
    ) -> Result<()> {
        self.run(
            "ecs",
            "AuthorizeSecurityGroup",
            &[
                ("RegionId", region.to_string()),
                ("SecurityGroupId", sg_id.to_string()),
                ("IpProtocol", protocol.to_string()),
                ("PortRange", port_range.to_string()),
                ("SourceCidrIp", cidr.to_string()),
            ],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Key Pair
    // ------------------------------------------------------------------

    pub fn key_pair_exists(&self, region: &str, name: &str) -> Result<bool> {
        let res = self.run(
            "ecs",
            "DescribeKeyPairs",
            &[
                ("RegionId", region.to_string()),
                ("KeyPairName", name.to_string()),
            ],
        )?;
        Ok(res["KeyPairs"]["KeyPair"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false))
    }

    /// Create a key pair and return the private key PEM.
    pub fn create_key_pair(&self, region: &str, name: &str) -> Result<String> {
        let res = self.run(
            "ecs",
            "CreateKeyPair",
            &[
                ("RegionId", region.to_string()),
                ("KeyPairName", name.to_string()),
            ],
        )?;
        res["PrivateKeyBody"]
            .as_str()
            .map(String::from)
            .context("CreateKeyPair response missing PrivateKeyBody")
    }

    // ------------------------------------------------------------------
    // ECS Instance
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn run_instance(
        &self,
        region: &str,
        zone: &str,
        image_id: &str,
        instance_type: &str,
        vswitch_id: &str,
        sg_id: &str,
        key_pair_name: &str,
        name: &str,
    ) -> Result<String> {
        let mut args = vec![
            ("RegionId", region.to_string()),
            ("ZoneId", zone.to_string()),
            ("ImageId", image_id.to_string()),
            ("InstanceType", instance_type.to_string()),
            ("VSwitchId", vswitch_id.to_string()),
            ("SecurityGroupId", sg_id.to_string()),
            ("KeyPairName", key_pair_name.to_string()),
            ("InstanceName", name.to_string()),
            ("InstanceChargeType", "PostPaid".to_string()),
            ("InternetChargeType", "PayByTraffic".to_string()),
            ("InternetMaxBandwidthOut", "5".to_string()),
            ("SystemDisk.Category", "cloud_efficiency".to_string()),
            ("SystemDisk.Size", "40".to_string()),
        ];
        add_tags(&mut args, name);
        let res = self.run("ecs", "RunInstances", &args)?;
        let ids = res["InstanceIdSets"]["InstanceIdSet"]
            .as_array()
            .context("RunInstances did not return InstanceIdSet")?;
        ids.first()
            .and_then(|v| v.as_str())
            .map(String::from)
            .context("RunInstances returned empty InstanceIdSet")
    }

    pub fn find_instance(&self, region: &str, name: &str) -> Result<Option<InstanceInfo>> {
        let res = self.run(
            "ecs",
            "DescribeInstances",
            &[
                ("RegionId", region.to_string()),
                ("InstanceName", name.to_string()),
            ],
        )?;
        let instance = res["Instances"]["Instance"]
            .as_array()
            .and_then(|arr| arr.first());
        if let Some(inst) = instance {
            let public_ip = inst["PublicIpAddress"]["IpAddress"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| inst["EipAddress"]["Ip"].as_str().map(String::from));
            Ok(Some(InstanceInfo {
                instance_id: inst["InstanceId"]
                    .as_str()
                    .context("DescribeInstances missing InstanceId")?
                    .to_string(),
                status: inst["Status"]
                    .as_str()
                    .context("DescribeInstances missing Status")?
                    .to_string(),
                public_ip,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn wait_for_running(
        &self,
        region: &str,
        name: &str,
        timeout: Duration,
    ) -> Result<InstanceInfo> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(info) = self.find_instance(region, name)? {
                if info.status == "Running" {
                    if info.public_ip.is_some() {
                        return Ok(info);
                    }
                    // Running but public IP not yet assigned; keep polling.
                } else if ["Stopped", "Stopping", "Deleted"].contains(&info.status.as_str()) {
                    bail!(
                        "instance entered terminal/non-recoverable state: {}",
                        info.status
                    );
                }
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for instance to become Running");
            }
            thread::sleep(Duration::from_secs(5));
        }
    }

    pub fn start_instance(&self, region: &str, instance_id: &str) -> Result<()> {
        self.run(
            "ecs",
            "StartInstance",
            &[
                ("RegionId", region.to_string()),
                ("InstanceId", instance_id.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn delete_instance(&self, region: &str, instance_id: &str) -> Result<()> {
        self.run(
            "ecs",
            "DeleteInstance",
            &[
                ("RegionId", region.to_string()),
                ("InstanceId", instance_id.to_string()),
                ("Force", "true".to_string()),
            ],
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub status: String,
    pub public_ip: Option<String>,
}

/// Save a PEM private key to a file with restrictive permissions.
pub fn save_private_key<P: AsRef<Path>>(path: P, pem: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(pem.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod {}", path.display()))?;
    Ok(())
}

fn add_tags(args: &mut Vec<(&str, String)>, name: &str) {
    args.push(("Tag.1.Key", "Project".to_string()));
    args.push(("Tag.1.Value", "octopus-qqbot".to_string()));
    args.push(("Tag.2.Key", "Name".to_string()));
    args.push(("Tag.2.Value", name.to_string()));
}
