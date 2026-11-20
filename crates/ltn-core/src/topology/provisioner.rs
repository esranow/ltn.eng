use crate::crypto::hash::StateHasher;
use crate::topology::kernel_tuner::{KernelPerformanceProfile, LiveKernelTuner};
use crate::topology::map_mtl_bridge::MapMtlBridge;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InfraTarget {
    BareMetal,
    LocalHost,
    AwsVpc,
    GcpVpc,
    AzureVnet,
}

impl std::str::FromStr for InfraTarget {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bare-metal" | "baremetal" | "metal" => Ok(InfraTarget::BareMetal),
            "localhost" | "host" | "local" => Ok(InfraTarget::LocalHost),
            "aws" | "aws-vpc" => Ok(InfraTarget::AwsVpc),
            "gcp" | "gcp-vpc" => Ok(InfraTarget::GcpVpc),
            "azure" | "azure-vnet" => Ok(InfraTarget::AzureVnet),
            _ => Err(format!(
                "Unknown infra target '{}'. Valid options: bare-metal, localhost, aws, gcp, azure",
                s
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkProvisioningSpec {
    pub subnet_cidr: String,
    pub bridge_interface: String,
    pub gateway_ip: String,
    pub dns_servers: Vec<String>,
    pub isolated_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProvisioningSpec {
    pub base_storage_path: String,
    pub volume_mounts: Vec<String>,
    pub quota_limit_gb: u64,
    pub filesystem_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeProvisioningSpec {
    pub cpu_governor: String,
    pub c_state_latency_us: u32,
    pub service_daemon_unit: String,
    pub allocated_cores: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraProvisioningReceipt {
    pub target: InfraTarget,
    pub status: String,
    pub dry_run: bool,
    pub sha256_hash: String,
    pub network: NetworkProvisioningSpec,
    pub storage: StorageProvisioningSpec,
    pub compute: ComputeProvisioningSpec,
    pub generated_artifacts: Vec<String>,
    pub message: String,
}

pub struct MapMtlInfraProvisioner;

impl MapMtlInfraProvisioner {
    /// Provisions bare-metal or local host base infrastructure:
    /// - Stage storage pools & volume directory hierarchy
    /// - Stage virtual network bridge & isolation configuration
    /// - Generate systemd/Windows daemon service manifests
    /// - Execute hardware kernel tuning based on physical topology
    pub fn provision_host(
        storage_root: &Path,
        dry_run: bool,
        profile: KernelPerformanceProfile,
    ) -> Result<InfraProvisioningReceipt, String> {
        let topology = MapMtlBridge::discover_topology()?;
        Self::provision_host_with_topology(storage_root, dry_run, profile, &topology)
    }

    /// Same as `provision_host`, with a hardware graph the caller already has
    /// (e.g. from `MapMtlBridge::graph_from_scan`).
    pub fn provision_host_with_topology(
        storage_root: &Path,
        dry_run: bool,
        profile: KernelPerformanceProfile,
        topology: &crate::topology::MtlHostGraphSchema,
    ) -> Result<InfraProvisioningReceipt, String> {
        // Tune the first (up to) 8 logical CPUs map-mtl reported.
        let core_ids: Vec<u32> = topology
            .nodes
            .iter()
            .filter_map(|n| match n.node_type {
                crate::topology::MtlNodeType::Thread(cpu) => Some(cpu),
                _ => None,
            })
            .take(8)
            .collect();

        // 1. Kernel Hardware Tuning
        let tuning_receipt = LiveKernelTuner::tune_hardware(profile, &core_ids, dry_run);

        // 2. Storage Pool Hierarchies
        let vol_dir = storage_root.join("volumes");
        let data_dir = storage_root.join("data");
        let net_dir = storage_root.join("network");
        let srv_dir = storage_root.join("services");

        let mut generated_artifacts = Vec::new();

        if !dry_run {
            std::fs::create_dir_all(&vol_dir)
                .map_err(|e| format!("Failed to create volumes dir: {}", e))?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("Failed to create data dir: {}", e))?;
            std::fs::create_dir_all(&net_dir)
                .map_err(|e| format!("Failed to create network dir: {}", e))?;
            std::fs::create_dir_all(&srv_dir)
                .map_err(|e| format!("Failed to create services dir: {}", e))?;
        }

        generated_artifacts.push(vol_dir.display().to_string());
        generated_artifacts.push(data_dir.display().to_string());

        // 3. Network Bridge Spec
        let net_spec = NetworkProvisioningSpec {
            subnet_cidr: "10.42.0.0/16".to_string(),
            bridge_interface: "ltn0".to_string(),
            gateway_ip: "10.42.0.1".to_string(),
            dns_servers: vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()],
            isolated_ports: vec![80, 443, 50051, 9090],
        };

        let net_config_path = net_dir.join("ltn-bridge.json");
        let net_json = serde_json::to_string_pretty(&net_spec).map_err(|e| e.to_string())?;
        if !dry_run {
            std::fs::write(&net_config_path, &net_json)
                .map_err(|e| format!("Failed to write network config: {}", e))?;
        }
        generated_artifacts.push(net_config_path.display().to_string());

        // 4. Compute Daemon Service Manifest
        let service_manifest = format!(
            "[Unit]\n\
             Description=MAP-MTL Hardware & Orchestration Daemon\n\
             After=network.target\n\n\
             [Service]\n\
             Type=simple\n\
             Restart=always\n\
             RestartSec=3s\n\
             CPUAccounting=true\n\
             CPUShares=1024\n\
             ExecStart=/usr/local/bin/ltn daemon --storage-root {}\n\n\
             [Install]\n\
             WantedBy=multi-user.target\n",
            storage_root.display()
        );

        let service_file_path = srv_dir.join("ltn-daemon.service");
        if !dry_run {
            std::fs::write(&service_file_path, &service_manifest)
                .map_err(|e| format!("Failed to write service unit: {}", e))?;
        }
        generated_artifacts.push(service_file_path.display().to_string());

        let storage_spec = StorageProvisioningSpec {
            base_storage_path: storage_root.display().to_string(),
            volume_mounts: vec!["volumes".to_string(), "data".to_string()],
            quota_limit_gb: 500,
            filesystem_type: "ext4/zfs/ntfs".to_string(),
        };

        let compute_spec = ComputeProvisioningSpec {
            cpu_governor: tuning_receipt.cpu_governor,
            c_state_latency_us: tuning_receipt.c_state_latency_us,
            service_daemon_unit: "ltn-daemon.service".to_string(),
            allocated_cores: core_ids,
        };

        let hash_input = format!(
            "{}:{}:{}:{}",
            net_spec.subnet_cidr,
            storage_spec.base_storage_path,
            compute_spec.cpu_governor,
            generated_artifacts.len()
        );
        let sha256_hash = StateHasher::hash_str(&hash_input);

        Ok(InfraProvisioningReceipt {
            target: InfraTarget::BareMetal,
            status: if dry_run {
                "VERIFIED_DRY_RUN".to_string()
            } else {
                "PROVISIONED_SUCCESS".to_string()
            },
            dry_run,
            sha256_hash,
            network: net_spec,
            storage: storage_spec,
            compute: compute_spec,
            generated_artifacts,
            message: format!(
                "MAP-MTL infra provisioned on bare metal. Topology: {} nodes discovered. Kernel governor pinned.",
                topology.nodes.len()
            ),
        })
    }

    /// Provisions base Cloud Infrastructure (VPC, Subnets, Gateways, Route Tables, Security Groups)
    /// Outputs production-ready Terraform / OpenTofu HCL code.
    pub fn provision_cloud(
        target: InfraTarget,
        output_dir: &Path,
        dry_run: bool,
    ) -> Result<InfraProvisioningReceipt, String> {
        let (provider_str, hcl_code) = match target {
            InfraTarget::AwsVpc => ("aws", Self::generate_aws_vpc_hcl()),
            InfraTarget::GcpVpc => ("gcp", Self::generate_gcp_vpc_hcl()),
            InfraTarget::AzureVnet => ("azure", Self::generate_azure_vnet_hcl()),
            _ => return Err("Invalid cloud target for cloud provisioning".to_string()),
        };

        let mut generated_artifacts = Vec::new();
        let tf_file = output_dir.join(format!("{}_base_infra.tf", provider_str));

        if !dry_run {
            std::fs::create_dir_all(output_dir)
                .map_err(|e| format!("Failed to create cloud output dir: {}", e))?;
            std::fs::write(&tf_file, &hcl_code)
                .map_err(|e| format!("Failed to write Terraform file: {}", e))?;
        }
        generated_artifacts.push(tf_file.display().to_string());

        let net_spec = NetworkProvisioningSpec {
            subnet_cidr: "10.0.0.0/16".to_string(),
            bridge_interface: "cloud-vpc".to_string(),
            gateway_ip: "10.0.0.1".to_string(),
            dns_servers: vec!["AmazonProvidedDNS".to_string()],
            isolated_ports: vec![80, 443, 50051],
        };

        let storage_spec = StorageProvisioningSpec {
            base_storage_path: format!("{}-s3-ebs-storage", provider_str),
            volume_mounts: vec!["gp3-encrypted-root".to_string()],
            quota_limit_gb: 1000,
            filesystem_type: "cloud-block-storage".to_string(),
        };

        let compute_spec = ComputeProvisioningSpec {
            cpu_governor: "cloud-hypervisor".to_string(),
            c_state_latency_us: 0,
            service_daemon_unit: "ecs-fargate-k8s-agent".to_string(),
            allocated_cores: vec![0, 1, 2, 3],
        };

        let sha256_hash = StateHasher::hash_str(&hcl_code);

        let (status, message) = if dry_run {
            (
                "VERIFIED_DRY_RUN".to_string(),
                format!(
                    "Generated production base cloud infrastructure HCL for {}",
                    provider_str
                ),
            )
        } else {
            match Self::check_iac_binary() {
                None => (
                    "IAC_UNAVAILABLE".to_string(),
                    "IAC_BINARY_MISSING: Neither 'tofu' nor 'terraform' found on PATH. Install OpenTofu to provision cloud infrastructure.".to_string(),
                ),
                Some(bin) => {
                    let init_res = std::process::Command::new(&bin)
                        .current_dir(output_dir)
                        .args(["init", "-no-color"])
                        .output();

                    match init_res {
                        Ok(init_output) if init_output.status.success() => {
                            let apply_res = std::process::Command::new(&bin)
                                .current_dir(output_dir)
                                .args(["apply", "-auto-approve", "-no-color"])
                                .output();

                            match apply_res {
                                Ok(apply_output) if apply_output.status.success() => (
                                    "PROVISIONED_SUCCESS".to_string(),
                                    format!(
                                        "Successfully provisioned {} base cloud infrastructure using {}",
                                        provider_str, bin
                                    ),
                                ),
                                Ok(apply_output) => {
                                    let stderr = String::from_utf8_lossy(&apply_output.stderr);
                                    let stdout = String::from_utf8_lossy(&apply_output.stdout);
                                    let detail = if !stderr.trim().is_empty() {
                                        stderr.trim()
                                    } else {
                                        stdout.trim()
                                    };
                                    (
                                        "IAC_APPLY_FAILED".to_string(),
                                        format!("{} apply failed: {}", bin, detail),
                                    )
                                }
                                Err(e) => (
                                    "IAC_APPLY_FAILED".to_string(),
                                    format!("Failed to execute {} apply: {}", bin, e),
                                ),
                            }
                        }
                        Ok(init_output) => {
                            let stderr = String::from_utf8_lossy(&init_output.stderr);
                            let stdout = String::from_utf8_lossy(&init_output.stdout);
                            let detail = if !stderr.trim().is_empty() {
                                stderr.trim()
                            } else {
                                stdout.trim()
                            };
                            (
                                "IAC_INIT_FAILED".to_string(),
                                format!("{} init failed: {}", bin, detail),
                            )
                        }
                        Err(e) => (
                            "IAC_INIT_FAILED".to_string(),
                            format!("Failed to execute {} init: {}", bin, e),
                        ),
                    }
                }
            }
        };

        Ok(InfraProvisioningReceipt {
            target,
            status,
            dry_run,
            sha256_hash,
            network: net_spec,
            storage: storage_spec,
            compute: compute_spec,
            generated_artifacts,
            message,
        })
    }

    /// Checks if `tofu` or `terraform` binary exists on PATH.
    pub fn check_iac_binary() -> Option<String> {
        for bin in &["tofu", "terraform"] {
            if let Ok(output) = std::process::Command::new(bin).arg("version").output() {
                if output.status.success() {
                    return Some((*bin).to_string());
                }
            }
            if let Ok(path_var) = std::env::var("PATH") {
                let extensions = if cfg!(windows) {
                    vec!["", ".exe", ".cmd", ".bat"]
                } else {
                    vec![""]
                };
                for dir in std::env::split_paths(&path_var) {
                    for ext in &extensions {
                        let candidate = dir.join(format!("{}{}", bin, ext));
                        if candidate.is_file() {
                            return Some((*bin).to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn generate_aws_vpc_hcl() -> String {
        r#"# MAP-MTL Synthesized Base Cloud Infrastructure (AWS)
terraform {
  required_version = ">= 1.5.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

variable "aws_region" {
  type    = string
  default = "us-east-1"
}

resource "aws_vpc" "ltn_base_vpc" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags = {
    Name        = "ltn-base-vpc"
    Provisioner = "map.mtl"
  }
}

resource "aws_internet_gateway" "ltn_igw" {
  vpc_id = aws_vpc.ltn_base_vpc.id
  tags = {
    Name = "ltn-igw"
  }
}

resource "aws_subnet" "ltn_public_1a" {
  vpc_id                  = aws_vpc.ltn_base_vpc.id
  cidr_block              = "10.0.1.0/24"
  availability_zone       = "${var.aws_region}a"
  map_public_ip_on_launch = true
  tags = {
    Name = "ltn-public-subnet-1a"
  }
}

resource "aws_subnet" "ltn_public_1b" {
  vpc_id                  = aws_vpc.ltn_base_vpc.id
  cidr_block              = "10.0.2.0/24"
  availability_zone       = "${var.aws_region}b"
  map_public_ip_on_launch = true
  tags = {
    Name = "ltn-public-subnet-1b"
  }
}

resource "aws_route_table" "ltn_public_rt" {
  vpc_id = aws_vpc.ltn_base_vpc.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.ltn_igw.id
  }
}

resource "aws_route_table_association" "ltn_rta_1a" {
  subnet_id      = aws_subnet.ltn_public_1a.id
  route_table_id = aws_route_table.ltn_public_rt.id
}

resource "aws_security_group" "ltn_mesh_sg" {
  name        = "ltn-mesh-sg"
  description = "MAP-MTL Zero-Trust Orchestration Security Group"
  vpc_id      = aws_vpc.ltn_base_vpc.id

  ingress {
    description = "TLS Ingress"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    description = "Internal gRPC Mesh"
    from_port   = 50051
    to_port     = 50051
    protocol    = "tcp"
    self        = true
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}
"#
        .to_string()
    }

    fn generate_gcp_vpc_hcl() -> String {
        r#"# MAP-MTL Synthesized Base Cloud Infrastructure (GCP)
terraform {
  required_version = ">= 1.5.0"
  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 5.0"
    }
  }
}

resource "google_compute_network" "ltn_vpc" {
  name                    = "ltn-base-vpc"
  auto_create_subnetworks = false
}

resource "google_compute_subnetwork" "ltn_subnetwork" {
  name          = "ltn-subnet-central"
  ip_cidr_range = "10.0.0.0/20"
  region        = "us-central1"
  network       = google_compute_network.ltn_vpc.id
}

resource "google_compute_firewall" "ltn_ingress" {
  name    = "ltn-allow-mesh"
  network = google_compute_network.ltn_vpc.name

  allow {
    protocol = "tcp"
    ports    = ["443", "50051", "9090"]
  }
  source_ranges = ["0.0.0.0/0"]
}
"#
        .to_string()
    }

    fn generate_azure_vnet_hcl() -> String {
        r#"# MAP-MTL Synthesized Base Cloud Infrastructure (Azure)
terraform {
  required_version = ">= 1.5.0"
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 3.0"
    }
  }
}

resource "azurerm_resource_group" "ltn_rg" {
  name     = "ltn-infra-rg"
  location = "eastus"
}

resource "azurerm_virtual_network" "ltn_vnet" {
  name                = "ltn-base-vnet"
  address_space       = ["10.0.0.0/16"]
  location            = azurerm_resource_group.ltn_rg.location
  resource_group_name = azurerm_resource_group.ltn_rg.name
}

resource "azurerm_subnet" "ltn_subnet" {
  name                 = "ltn-default-subnet"
  resource_group_name  = azurerm_resource_group.ltn_rg.name
  virtual_network_name = azurerm_virtual_network.ltn_vnet.name
  address_prefixes     = ["10.0.1.0/24"]
}
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provision_bare_metal_dry_run() {
        let temp_dir = std::env::temp_dir().join("map_mtl_test_dry_run");
        // Real `map-mtl scan --json` output captured on a Linux host.
        let scan = MapMtlBridge::parse_scan(include_bytes!("../../tests/fixtures/map_mtl/linux_scan.json"))
            .expect("fixture parses");
        let topology = MapMtlBridge::graph_from_scan(&scan);
        let receipt = MapMtlInfraProvisioner::provision_host_with_topology(
            &temp_dir,
            true,
            KernelPerformanceProfile::Performance,
            &topology,
        )
        .expect("Dry run host provisioning must succeed");

        assert_eq!(receipt.target, InfraTarget::BareMetal);
        assert_eq!(receipt.status, "VERIFIED_DRY_RUN");
        assert!(receipt.dry_run);
        assert!(!receipt.sha256_hash.is_empty());
        assert_eq!(receipt.network.bridge_interface, "ltn0");
        assert_eq!(receipt.compute.cpu_governor, "performance");
    }

    #[test]
    fn test_provision_cloud_aws_dry_run() {
        let temp_dir = std::env::temp_dir().join("map_mtl_cloud_test");
        let receipt = MapMtlInfraProvisioner::provision_cloud(InfraTarget::AwsVpc, &temp_dir, true)
            .expect("AWS cloud provisioning must succeed");

        assert_eq!(receipt.target, InfraTarget::AwsVpc);
        assert_eq!(receipt.status, "VERIFIED_DRY_RUN");
        assert!(!receipt.sha256_hash.is_empty());
        assert!(receipt.generated_artifacts[0].ends_with("aws_base_infra.tf"));
    }

    #[test]
    fn test_provision_cloud_aws_not_dry_run_iac_status() {
        let temp_dir = std::env::temp_dir().join(format!("map_mtl_cloud_test_nodry_{}", uuid::Uuid::new_v4()));
        let receipt = MapMtlInfraProvisioner::provision_cloud(InfraTarget::AwsVpc, &temp_dir, false)
            .expect("Cloud provisioning receipt must be returned");

        assert_eq!(receipt.target, InfraTarget::AwsVpc);
        assert!(!receipt.dry_run);
        assert!(!receipt.sha256_hash.is_empty());
        assert!(receipt.generated_artifacts[0].ends_with("aws_base_infra.tf"));
        assert!(std::path::Path::new(&receipt.generated_artifacts[0]).exists());

        // When OpenTofu/Terraform is not installed, must return IAC_UNAVAILABLE honestly
        if MapMtlInfraProvisioner::check_iac_binary().is_none() {
            assert_eq!(receipt.status, "IAC_UNAVAILABLE");
            assert!(receipt.message.contains("IAC_BINARY_MISSING"));
        } else {
            assert!(
                receipt.status == "PROVISIONED_SUCCESS"
                    || receipt.status == "IAC_APPLY_FAILED"
                    || receipt.status == "IAC_INIT_FAILED"
            );
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
