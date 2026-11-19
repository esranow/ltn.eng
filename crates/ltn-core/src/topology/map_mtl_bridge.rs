use crate::topology::hardware_inventory::HostHardwareInventory;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MtlNodeType {
    Socket(u32),
    Core(u32),
    Thread(u32),
    Cache { level: u8 },
    NumaDomain(u32),
    PciDevice,
    GpuAccelerator,
    MemoryBank,
    Switch { mac: String, layer: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MtlEdgeType {
    SharedCache(u8),
    PhysicalProximity(u32),
    PciLink(u32),
    NumaDistance {
        latency_ns: u32,
        bandwidth_gbps: u32,
    },
    NvLink {
        bandwidth_gbps: u32,
    },
    NetworkLink {
        protocol: String,
        bandwidth_gbps: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtlGraphNode {
    pub id: u32,
    pub node_type: MtlNodeType,
    pub available_capacity: u64,
    pub allocated_capacity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtlHostGraphSchema {
    pub nodes: Vec<MtlGraphNode>,
    pub edges: Vec<(u32, u32, MtlEdgeType)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvLinkMatrix {
    pub links: Vec<NvLinkConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvLinkConnection {
    pub gpu_source: u32,
    pub gpu_target: u32,
    pub bandwidth_gbps: u32,
    pub is_direct_p2p: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapMtlBridge;

impl MapMtlBridge {
    /// Discovers host topology using map-mtl binary or falls back to native system inspection
    pub fn discover_topology() -> MtlHostGraphSchema {
        // Attempt to call map-mtl if available on PATH or target/debug
        if let Some(schema) = Self::try_exec_map_mtl() {
            return schema;
        }

        // Native fallback synthesizer using real host sysinfo
        Self::synthesize_from_host_inventory(&HostHardwareInventory::probe_local_system())
    }

    fn try_exec_map_mtl() -> Option<MtlHostGraphSchema> {
        let candidates = [
            "map-mtl",
            "D:\\ltn-eng-workspace\\target\\debug\\map-mtl.exe",
            "D:\\ltn-eng-workspace\\target\\release\\map-mtl.exe",
        ];

        for bin in &candidates {
            if Path::new(bin).exists() || which_exists(bin) {
                if let Ok(output) = Command::new(bin).arg("scan").arg("--json").output() {
                    if output.status.success() {
                        if let Ok(schema) =
                            serde_json::from_slice::<MtlHostGraphSchema>(&output.stdout)
                        {
                            return Some(schema);
                        }
                    }
                }
            }
        }
        None
    }

    /// Synthesizes a structured physical graph from HostHardwareInventory
    pub fn synthesize_from_host_inventory(inv: &HostHardwareInventory) -> MtlHostGraphSchema {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let mut node_counter = 0u32;

        // 1. Sockets
        for s in 0..inv.cpu.sockets {
            let socket_id = node_counter;
            node_counter += 1;
            nodes.push(MtlGraphNode {
                id: socket_id,
                node_type: MtlNodeType::Socket(s),
                available_capacity: 100,
                allocated_capacity: 0,
            });

            // Cores in this socket
            for c in 0..inv.cpu.physical_cores {
                let core_id = node_counter;
                node_counter += 1;
                nodes.push(MtlGraphNode {
                    id: core_id,
                    node_type: MtlNodeType::Core(c),
                    available_capacity: 100,
                    allocated_capacity: 0,
                });

                edges.push((socket_id, core_id, MtlEdgeType::PhysicalProximity(1)));

                // L3 Cache per 4 cores
                if c % 4 == 0 {
                    let cache_id = node_counter;
                    node_counter += 1;
                    nodes.push(MtlGraphNode {
                        id: cache_id,
                        node_type: MtlNodeType::Cache { level: 3 },
                        available_capacity: 32 * 1024 * 1024,
                        allocated_capacity: 0,
                    });
                    edges.push((core_id, cache_id, MtlEdgeType::SharedCache(3)));
                }
            }
        }

        // 2. NUMA domains
        for numa in &inv.numa {
            let numa_id = node_counter;
            node_counter += 1;
            nodes.push(MtlGraphNode {
                id: numa_id,
                node_type: MtlNodeType::NumaDomain(numa.node_id),
                available_capacity: numa.memory_mb,
                allocated_capacity: 0,
            });
        }

        // 3. GPU Accelerators & NVLink interlinks
        if inv.gpu.count > 0 {
            let mut gpu_node_ids = Vec::new();
            for _g in 0..inv.gpu.count {
                let gpu_id = node_counter;
                node_counter += 1;
                nodes.push(MtlGraphNode {
                    id: gpu_id,
                    node_type: MtlNodeType::GpuAccelerator,
                    available_capacity: inv.gpu.vram_mb_per_device,
                    allocated_capacity: 0,
                });
                gpu_node_ids.push(gpu_id);

                // Connect to PCIe
                edges.push((0, gpu_id, MtlEdgeType::PciLink(64))); // Gen 4/5 64 GT/s
            }

            // High-speed NVLink mesh if multi-GPU
            if gpu_node_ids.len() > 1 {
                for i in 0..gpu_node_ids.len() {
                    for j in (i + 1)..gpu_node_ids.len() {
                        edges.push((
                            gpu_node_ids[i],
                            gpu_node_ids[j],
                            MtlEdgeType::NvLink {
                                bandwidth_gbps: 900,
                            },
                        ));
                    }
                }
            }
        }

        MtlHostGraphSchema { nodes, edges }
    }

    /// Derives the high-speed GPU interconnect matrix
    pub fn get_nvlink_matrix(inv: &HostHardwareInventory) -> NvLinkMatrix {
        let mut links = Vec::new();
        if inv.gpu.count > 1 {
            for i in 0..inv.gpu.count {
                for j in (i + 1)..inv.gpu.count {
                    links.push(NvLinkConnection {
                        gpu_source: i,
                        gpu_target: j,
                        bandwidth_gbps: 900, // 900 GB/s bidirectional NVLink
                        is_direct_p2p: true,
                    });
                }
            }
        }
        NvLinkMatrix { links }
    }
}

fn which_exists(binary: &str) -> bool {
    Command::new(binary).arg("--version").output().is_ok()
}
