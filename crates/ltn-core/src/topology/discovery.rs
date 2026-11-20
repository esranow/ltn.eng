use crate::ir::ResourceBlock;
use crate::topology::hardware_inventory::HostHardwareInventory;
use crate::topology::map_mtl_bridge::{MapMtlBridge, MtlHostGraphSchema};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyDiscoveryAdapter {
    pub name: String,
}

impl Default for TopologyDiscoveryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TopologyDiscoveryAdapter {
    pub fn new() -> Self {
        Self {
            name: "MetalMapTopologyAdapter".to_string(),
        }
    }

    /// Discovers real local hardware via sysinfo and map-mtl hardware topology graph
    pub fn discover_local_hardware(&self) -> ResourceBlock {
        let inv = HostHardwareInventory::probe_local_system();

        ResourceBlock {
            block_id: Uuid::new_v4(),
            node_name: inv.hostname,
            total_cpu_cores: inv.cpu.logical_threads,
            total_ram_mb: inv.ram.total_mb as u32,
            total_vram_mb: inv.gpu.total_vram_mb as u32,
            total_gpu_count: inv.gpu.count,
            network_bw_gbps: 10,
            endpoint: "127.0.0.1:8080".to_string(),
        }
    }

    /// Hardware graph from `map-mtl scan --json`; an error if map-mtl is
    /// unavailable or its output cannot be read.
    pub fn discover_physical_graph(&self) -> Result<MtlHostGraphSchema, String> {
        MapMtlBridge::discover_topology()
    }
}
