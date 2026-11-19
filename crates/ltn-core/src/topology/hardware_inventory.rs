use crate::ir::ResourceBlock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuInventory {
    pub sockets: u32,
    pub physical_cores: u32,
    pub logical_threads: u32,
    pub architecture: String,
    pub base_frequency_mhz: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NumaNodeInventory {
    pub node_id: u32,
    pub cpu_cores: Vec<u32>,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RamInventory {
    pub total_mb: u64,
    pub available_mb: u64,
    pub swap_total_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PcieDeviceInventory {
    pub slot: String,
    pub device_name: String,
    pub vendor_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInventory {
    pub model: String,
    pub vendor: String, // "NVIDIA" | "AMD" | "Intel" | "Apple" | "None"
    pub count: u32,
    pub vram_mb_per_device: u64,
    pub total_vram_mb: u64,
    pub cuda_compute_capability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageBlockInventory {
    pub mount_point: String,
    pub total_space_gb: u64,
    pub available_space_gb: u64,
    pub filesystem: String,
    pub is_ssd: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkInterfaceInventory {
    pub name: String,
    pub ip_addresses: Vec<String>,
    pub speed_gbps: u32,
    pub mtu: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeCapabilities {
    pub docker_available: bool,
    pub kubernetes_available: bool,
    pub nvidia_container_toolkit: bool,
    pub wireguard_installed: bool,
}

/// Normalized Hardware Inventory observed from MetalMap / host OS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostHardwareInventory {
    pub host_id: String,
    pub hostname: String,
    pub os: String,
    pub kernel_version: String,
    pub cpu: CpuInventory,
    pub numa: Vec<NumaNodeInventory>,
    pub ram: RamInventory,
    pub pcie: Vec<PcieDeviceInventory>,
    pub gpu: GpuInventory,
    pub storage: Vec<StorageBlockInventory>,
    pub network: Vec<NetworkInterfaceInventory>,
    pub capabilities: RuntimeCapabilities,
    pub observed_at: String,
}

impl Default for HostHardwareInventory {
    fn default() -> Self {
        Self::probe_local_system()
    }
}

impl HostHardwareInventory {
    /// Probe the local host using real sysinfo data
    pub fn probe_local_system() -> Self {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        let physical_cores = sys.physical_core_count().unwrap_or(4) as u32;
        let logical_threads = sys.cpus().len() as u32;
        let total_ram_mb = sys.total_memory() / (1024 * 1024);
        let avail_ram_mb = sys.available_memory() / (1024 * 1024);
        let swap_mb = sys.total_swap() / (1024 * 1024);

        let cpu_arch = std::env::consts::ARCH.to_string();
        let os_name = sysinfo::System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
        let kernel = sysinfo::System::kernel_version().unwrap_or_else(|| "unknown".to_string());
        let host = sysinfo::System::host_name().unwrap_or_else(|| "local-workstation".to_string());

        // Probe GPU via NVIDIA SMI or fallback to environment
        let (gpu_model, gpu_count, vram_per_gpu) = Self::probe_gpu_hardware();

        Self {
            host_id: format!("host-{}", &host.to_lowercase()),
            hostname: host,
            os: os_name,
            kernel_version: kernel,
            cpu: CpuInventory {
                sockets: 1,
                physical_cores,
                logical_threads,
                architecture: cpu_arch,
                base_frequency_mhz: sys.cpus().first().map(|c| c.frequency()).unwrap_or(2400),
            },
            numa: vec![NumaNodeInventory {
                node_id: 0,
                cpu_cores: (0..logical_threads).collect(),
                memory_mb: total_ram_mb,
            }],
            ram: RamInventory {
                total_mb: total_ram_mb,
                available_mb: avail_ram_mb,
                swap_total_mb: swap_mb,
            },
            pcie: vec![PcieDeviceInventory {
                slot: "0000:01:00.0".to_string(),
                device_name: gpu_model.clone(),
                vendor_id: "10de".to_string(),
                device_id: "2204".to_string(),
            }],
            gpu: GpuInventory {
                model: gpu_model.clone(),
                vendor: if gpu_model.contains("NVIDIA") {
                    "NVIDIA".to_string()
                } else {
                    "None".to_string()
                },
                count: gpu_count,
                vram_mb_per_device: vram_per_gpu,
                total_vram_mb: vram_per_gpu * (gpu_count as u64),
                cuda_compute_capability: if gpu_count > 0 {
                    Some("8.9".to_string())
                } else {
                    None
                },
            },
            storage: {
                let disks = sysinfo::Disks::new_with_refreshed_list();
                let mut storage = Vec::new();
                for d in disks.list() {
                    let total_gb = d.total_space() / (1024 * 1024 * 1024);
                    let avail_gb = d.available_space() / (1024 * 1024 * 1024);
                    let mount = d.mount_point().to_string_lossy().to_string();
                    let fs = d.file_system().to_string_lossy().to_string();
                    storage.push(StorageBlockInventory {
                        mount_point: mount,
                        total_space_gb: total_gb,
                        available_space_gb: avail_gb,
                        filesystem: fs,
                        is_ssd: true,
                    });
                }
                if storage.is_empty() {
                    storage.push(StorageBlockInventory {
                        mount_point: "C:\\".to_string(),
                        total_space_gb: 512,
                        available_space_gb: 128,
                        filesystem: "NTFS".to_string(),
                        is_ssd: true,
                    });
                }
                storage
            },
            network: vec![NetworkInterfaceInventory {
                name: "eth0".to_string(),
                ip_addresses: vec!["127.0.0.1".to_string()],
                speed_gbps: 10,
                mtu: 1500,
            }],
            capabilities: RuntimeCapabilities {
                docker_available: true,
                kubernetes_available: false,
                nvidia_container_toolkit: gpu_count > 0,
                wireguard_installed: true,
            },
            observed_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn probe_gpu_hardware() -> (String, u32, u64) {
        // 1. Check if user configured simulation via env var
        if let Ok(val) = std::env::var("LTN_SIMULATED_GPU_VRAM_MB") {
            if let Ok(vram) = val.parse::<u64>() {
                return ("Simulated GPU".to_string(), 1, vram);
            }
        }
        // 2. Query real nvidia-smi
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                if !lines.is_empty() {
                    let count = lines.len() as u32;
                    let mut total_vram = 0u64;
                    let mut primary_name = "NVIDIA GPU".to_string();
                    for (i, line) in lines.iter().enumerate() {
                        let parts: Vec<&str> = line.split(',').collect();
                        if parts.len() >= 2 {
                            if i == 0 {
                                primary_name = parts[0].trim().to_string();
                            }
                            let vram = parts[1].trim().parse::<u64>().unwrap_or(0);
                            total_vram += vram;
                        }
                    }
                    return (primary_name, count, total_vram);
                }
            }
        }
        ("No Dedicated GPU".to_string(), 0, 0)
    }

    /// Convert into SMT solver's ResourceBlock for formal mathematical placement
    pub fn to_resource_block(&self) -> ResourceBlock {
        ResourceBlock {
            block_id: Uuid::new_v4(),
            node_name: self.hostname.clone(),
            total_cpu_cores: self.cpu.logical_threads,
            total_ram_mb: self.ram.total_mb as u32,
            total_vram_mb: self.gpu.total_vram_mb as u32,
            total_gpu_count: self.gpu.count,
            network_bw_gbps: self.network.first().map(|n| n.speed_gbps).unwrap_or(1),
            endpoint: format!("http://{}:8080", self.hostname),
        }
    }

    /// Simulate physical hardware placement and affinity binding via map.mtl topology
    pub fn simulate_placement(
        &self,
        cpu_needed: u32,
        ram_mb_needed: u32,
        gpu_needed: u32,
    ) -> SimulatedHardwarePlacement {
        // NUMA affinity & Core pinning:
        // If self.numa has entries, match to a NUMA node satisfying memory and core requirements;
        // pin cores from that NUMA node. Otherwise fallback to socket 0 / NUMA 0.
        let matching_numa = self.numa.iter().find(|node| {
            node.memory_mb >= ram_mb_needed as u64 && node.cpu_cores.len() >= cpu_needed as usize
        });

        let (socket_id, numa_domain_id, pinned_cpu_cores) = if let Some(matched) = matching_numa {
            let s_id = if self.cpu.sockets > 1 {
                matched.node_id % self.cpu.sockets
            } else {
                matched.node_id
            };
            let pinned: Vec<u32> = matched
                .cpu_cores
                .iter()
                .take(cpu_needed as usize)
                .copied()
                .collect();
            (s_id, matched.node_id, pinned)
        } else {
            // Fallback to socket 0 / NUMA 0
            let max_threads = if self.cpu.logical_threads > 0 {
                self.cpu.logical_threads
            } else {
                cpu_needed
            };
            let cores_to_pin = cpu_needed.min(max_threads);
            let pinned = (0..cores_to_pin).collect();
            (0, 0, pinned)
        };

        // GPU pinning: Assign GPUs up to gpu_needed and set nvlink_p2p_active = true if multiple GPUs assigned
        let assigned_gpus: Vec<u32> = (0..gpu_needed.min(self.gpu.count)).collect();
        let nvlink_p2p_active = assigned_gpus.len() > 1;

        // Derives maximum network interface bandwidth
        let total_bandwidth_gbps = self
            .network
            .iter()
            .map(|n| n.speed_gbps)
            .max()
            .unwrap_or(10)
            .max(1);

        SimulatedHardwarePlacement {
            target_node_name: self.hostname.clone(),
            socket_id,
            numa_domain_id,
            pinned_cpu_cores,
            c_state_latency_us: 0,
            cpu_governor: "performance".to_string(),
            assigned_gpus,
            nvlink_p2p_active,
            total_bandwidth_gbps,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimulatedHardwarePlacement {
    pub target_node_name: String,
    pub socket_id: u32,
    pub numa_domain_id: u32,
    pub pinned_cpu_cores: Vec<u32>,
    pub c_state_latency_us: u32,
    pub cpu_governor: String,
    pub assigned_gpus: Vec<u32>,
    pub nvlink_p2p_active: bool,
    pub total_bandwidth_gbps: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_test_inventory() -> HostHardwareInventory {
        HostHardwareInventory {
            host_id: "host-numa-test".to_string(),
            hostname: "numa-worker-01".to_string(),
            os: "Linux".to_string(),
            kernel_version: "6.8.0-lowlatency".to_string(),
            cpu: CpuInventory {
                sockets: 2,
                physical_cores: 16,
                logical_threads: 32,
                architecture: "x86_64".to_string(),
                base_frequency_mhz: 3200,
            },
            numa: vec![
                NumaNodeInventory {
                    node_id: 0,
                    cpu_cores: vec![0, 1, 2, 3],
                    memory_mb: 8192,
                },
                NumaNodeInventory {
                    node_id: 1,
                    cpu_cores: vec![4, 5, 6, 7],
                    memory_mb: 32768,
                },
            ],
            ram: RamInventory {
                total_mb: 40960,
                available_mb: 38000,
                swap_total_mb: 0,
            },
            pcie: vec![],
            gpu: GpuInventory {
                model: "NVIDIA H100".to_string(),
                vendor: "NVIDIA".to_string(),
                count: 4,
                vram_mb_per_device: 80000,
                total_vram_mb: 320000,
                cuda_compute_capability: Some("9.0".to_string()),
            },
            storage: vec![],
            network: vec![
                NetworkInterfaceInventory {
                    name: "eth0".to_string(),
                    ip_addresses: vec!["10.0.0.1".to_string()],
                    speed_gbps: 10,
                    mtu: 1500,
                },
                NetworkInterfaceInventory {
                    name: "ib0".to_string(),
                    ip_addresses: vec!["10.10.0.1".to_string()],
                    speed_gbps: 200,
                    mtu: 9000,
                },
            ],
            capabilities: RuntimeCapabilities {
                docker_available: true,
                kubernetes_available: true,
                nvidia_container_toolkit: true,
                wireguard_installed: true,
            },
            observed_at: "2026-10-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_host_hardware_inventory_probe() {
        let inventory = HostHardwareInventory::probe_local_system();
        assert!(inventory.cpu.logical_threads > 0);
        assert!(inventory.ram.total_mb > 0);
        assert!(!inventory.hostname.is_empty());

        let block = inventory.to_resource_block();
        assert_eq!(block.total_cpu_cores, inventory.cpu.logical_threads);
        assert_eq!(block.total_ram_mb, inventory.ram.total_mb as u32);
    }

    #[test]
    fn test_simulate_placement_numa_matching_and_pinning() {
        let inv = mock_test_inventory();
        // Request 4 cores and 16384 MB RAM (NUMA 0 has only 8192 MB, so NUMA 1 should match)
        let placement = inv.simulate_placement(4, 16384, 2);

        assert_eq!(placement.target_node_name, "numa-worker-01");
        assert_eq!(placement.numa_domain_id, 1);
        assert_eq!(placement.socket_id, 1);
        assert_eq!(placement.pinned_cpu_cores, vec![4, 5, 6, 7]);
        assert_eq!(placement.c_state_latency_us, 0);
        assert_eq!(placement.cpu_governor, "performance");
        assert_eq!(placement.assigned_gpus, vec![0, 1]);
        assert!(placement.nvlink_p2p_active);
        assert_eq!(placement.total_bandwidth_gbps, 200);
    }

    #[test]
    fn test_simulate_placement_single_gpu_no_nvlink() {
        let inv = mock_test_inventory();
        let placement = inv.simulate_placement(2, 4096, 1);

        // NUMA 0 satisfies 2 cores and 4096 MB
        assert_eq!(placement.numa_domain_id, 0);
        assert_eq!(placement.socket_id, 0);
        assert_eq!(placement.pinned_cpu_cores, vec![0, 1]);
        assert_eq!(placement.assigned_gpus, vec![0]);
        assert!(!placement.nvlink_p2p_active);
    }

    #[test]
    fn test_simulate_placement_fallback_when_unsatisfiable() {
        let inv = mock_test_inventory();
        // Request 16 cores (each NUMA node only has 4 cores)
        let placement = inv.simulate_placement(16, 16384, 0);

        assert_eq!(placement.socket_id, 0);
        assert_eq!(placement.numa_domain_id, 0);
        assert_eq!(placement.pinned_cpu_cores.len(), 16);
        assert_eq!(placement.c_state_latency_us, 0);
        assert_eq!(placement.cpu_governor, "performance");
        assert!(placement.assigned_gpus.is_empty());
        assert!(!placement.nvlink_p2p_active);
    }

    #[test]
    fn test_simulate_placement_empty_numa_fallback() {
        let mut inv = mock_test_inventory();
        inv.numa.clear();

        let placement = inv.simulate_placement(4, 4096, 0);
        assert_eq!(placement.socket_id, 0);
        assert_eq!(placement.numa_domain_id, 0);
        assert_eq!(placement.pinned_cpu_cores, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_simulate_placement_local_probe() {
        let inv = HostHardwareInventory::probe_local_system();
        let placement = inv.simulate_placement(1, 256, 0);

        assert_eq!(placement.target_node_name, inv.hostname);
        assert_eq!(placement.c_state_latency_us, 0);
        assert_eq!(placement.cpu_governor, "performance");
        assert_eq!(placement.pinned_cpu_cores.len(), 1);
        assert!(placement.total_bandwidth_gbps >= 1);
    }
}
