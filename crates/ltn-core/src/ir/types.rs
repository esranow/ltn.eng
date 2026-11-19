use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkloadKind {
    StaticWeb,
    NodePostgresApi,
    PythonVllmGpu,
    GoWorker,
    RustWasm,
    StatefulStore,
    GenericContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workload {
    pub id: Uuid,
    pub name: String,
    pub kind: WorkloadKind,
    pub containers: Vec<String>,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constraint {
    CpuCores(u32),
    RamMb(u32),
    VramMb(u32),
    GpuCount(u32),
    NetworkGbps(u32),
    WasiNetworkEdge(u16),
    DataLocality {
        data_block_id: Uuid,
        max_latency_ms: u32,
    },
    StrictIsolation(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBlock {
    pub block_id: Uuid,
    pub node_name: String,
    pub total_cpu_cores: u32,
    pub total_ram_mb: u32,
    pub total_vram_mb: u32,
    pub total_gpu_count: u32,
    pub network_bw_gbps: u32,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: Uuid,
    pub workloads: Vec<Workload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterState {
    pub resources: Vec<ResourceBlock>,
    pub network_latencies: HashMap<(Uuid, Uuid), u32>,
}
