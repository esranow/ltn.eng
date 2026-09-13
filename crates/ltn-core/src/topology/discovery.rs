use crate::ir::ResourceBlock;
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
            name: "LocalTopologyAdapter".to_string(),
        }
    }

    pub fn discover_local_hardware(&self) -> ResourceBlock {
        ResourceBlock {
            block_id: Uuid::new_v4(),
            node_name: "local-node".to_string(),
            total_cpu_cores: 8,
            total_ram_mb: 16384,
            total_vram_mb: 8192,
            total_gpu_count: 1,
            network_bw_gbps: 10,
            endpoint: "127.0.0.1:8080".to_string(),
        }
    }
}
