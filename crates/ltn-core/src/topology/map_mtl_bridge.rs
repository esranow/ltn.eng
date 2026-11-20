use serde::{Deserialize, Serialize};
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

/// Output of `map-mtl scan --json` (schema_version 1). Only the fields the
/// graph needs are read; unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct MapMtlScan {
    pub schema_version: u32,
    #[serde(default)]
    pub hostname: String,
    pub cpu: ScanCpu,
    pub memory: ScanMemory,
    #[serde(default)]
    pub gpus: Vec<ScanGpu>,
    #[serde(default)]
    pub numa_nodes: Vec<ScanNumaNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanCpu {
    #[serde(default)]
    pub physical_cores: Option<u32>,
    pub logical_threads: u32,
    #[serde(default)]
    pub sockets: Option<u32>,
    #[serde(default)]
    pub socket_topology: Vec<ScanSocket>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanSocket {
    pub id: u32,
    #[serde(default)]
    pub cpus: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanMemory {
    pub total_mb: u64,
    #[serde(default)]
    pub available_mb: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanGpu {
    #[serde(default)]
    pub vendor: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub vram_mb: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanNumaNode {
    pub id: u32,
    #[serde(default)]
    pub cpus: Vec<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
}

/// The only map-mtl output schema this bridge understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapMtlBridge;

impl MapMtlBridge {
    /// Runs `map-mtl scan --json` and turns it into a hardware graph.
    ///
    /// The binary is `LTN_MAP_MTL_BIN` if set, otherwise `map-mtl` on PATH.
    /// There is no fallback: if map-mtl is missing, fails, or prints output
    /// this bridge cannot read, the error says so instead of inventing a
    /// topology.
    pub fn discover_topology() -> Result<MtlHostGraphSchema, String> {
        let bin = std::env::var("LTN_MAP_MTL_BIN")
            .ok()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| "map-mtl".to_string());
        let output = Command::new(&bin).args(["scan", "--json"]).output().map_err(|e| {
            format!("could not run `{bin} scan --json`: {e}. Install map-mtl or set LTN_MAP_MTL_BIN")
        })?;
        if !output.status.success() {
            return Err(format!(
                "`{bin} scan --json` exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let scan = Self::parse_scan(&output.stdout)?;
        Ok(Self::graph_from_scan(&scan))
    }

    /// Parses `map-mtl scan --json` output, rejecting other schema versions.
    pub fn parse_scan(bytes: &[u8]) -> Result<MapMtlScan, String> {
        let scan: MapMtlScan =
            serde_json::from_slice(bytes).map_err(|e| format!("unreadable map-mtl scan output: {e}"))?;
        if scan.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(format!(
                "map-mtl scan schema_version {} is not supported (expected {SUPPORTED_SCHEMA_VERSION})",
                scan.schema_version
            ));
        }
        if scan.cpu.logical_threads == 0 {
            return Err("map-mtl scan reported 0 logical CPUs".to_string());
        }
        Ok(scan)
    }

    /// Builds the graph from what map-mtl measured. Nothing is added that the
    /// scan did not report: no assumed caches, sockets or GPU links.
    pub fn graph_from_scan(scan: &MapMtlScan) -> MtlHostGraphSchema {
        let mut nodes: Vec<MtlGraphNode> = Vec::new();
        let mut edges = Vec::new();
        fn push(nodes: &mut Vec<MtlGraphNode>, node_type: MtlNodeType, cap: u64) -> u32 {
            let id = nodes.len() as u32;
            nodes.push(MtlGraphNode { id, node_type, available_capacity: cap, allocated_capacity: 0 });
            id
        }

        // Logical CPUs, one Thread node each, keyed by OS cpu number.
        let mut thread_ids = std::collections::BTreeMap::new();
        for cpu in 0..scan.cpu.logical_threads {
            thread_ids.insert(cpu, push(&mut nodes, MtlNodeType::Thread(cpu), 1));
        }
        // Sockets only where the scan reported which CPUs belong to them.
        for sock in &scan.cpu.socket_topology {
            let sid = push(&mut nodes, MtlNodeType::Socket(sock.id), sock.cpus.len() as u64);
            for c in &sock.cpus {
                if let Some(t) = thread_ids.get(c) {
                    edges.push((sid, *t, MtlEdgeType::PhysicalProximity(1)));
                }
            }
        }
        // Memory: per NUMA node when reported, else one bank of total memory.
        if scan.numa_nodes.is_empty() {
            push(&mut nodes, MtlNodeType::MemoryBank, scan.memory.total_mb);
        } else {
            for n in &scan.numa_nodes {
                let nid = push(&mut nodes, MtlNodeType::NumaDomain(n.id), n.memory_mb.unwrap_or(0));
                for c in &n.cpus {
                    if let Some(t) = thread_ids.get(c) {
                        edges.push((nid, *t, MtlEdgeType::PhysicalProximity(0)));
                    }
                }
            }
        }
        // GPUs with their reported VRAM (0 when the scan could not read it).
        for g in &scan.gpus {
            push(&mut nodes, MtlNodeType::GpuAccelerator, g.vram_mb.unwrap_or(0));
        }
        MtlHostGraphSchema { nodes, edges }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: &str = include_str!("../../tests/fixtures/map_mtl/windows_scan.json");
    const LINUX: &str = include_str!("../../tests/fixtures/map_mtl/linux_scan.json");

    fn count(g: &MtlHostGraphSchema, f: impl Fn(&MtlNodeType) -> bool) -> usize {
        g.nodes.iter().filter(|n| f(&n.node_type)).count()
    }

    #[test]
    fn windows_scan_builds_threads_memory_and_gpus_only() {
        let scan = MapMtlBridge::parse_scan(WINDOWS.as_bytes()).unwrap();
        let g = MapMtlBridge::graph_from_scan(&scan);
        assert_eq!(count(&g, |t| matches!(t, MtlNodeType::Thread(_))), scan.cpu.logical_threads as usize);
        // The Windows scan has no socket topology: no sockets or caches are invented.
        assert_eq!(count(&g, |t| matches!(t, MtlNodeType::Socket(_))), 0);
        assert_eq!(count(&g, |t| matches!(t, MtlNodeType::Cache { .. })), 0);
        let banks: Vec<_> = g.nodes.iter().filter(|n| n.node_type == MtlNodeType::MemoryBank).collect();
        assert_eq!(banks.len(), 1);
        assert_eq!(banks[0].available_capacity, scan.memory.total_mb);
        assert_eq!(count(&g, |t| *t == MtlNodeType::GpuAccelerator), scan.gpus.len());
        assert!(!g.edges.iter().any(|e| matches!(e.2, MtlEdgeType::NvLink { .. })));
    }

    #[test]
    fn linux_scan_links_threads_to_socket_and_numa() {
        let scan = MapMtlBridge::parse_scan(LINUX.as_bytes()).unwrap();
        let g = MapMtlBridge::graph_from_scan(&scan);
        assert_eq!(count(&g, |t| matches!(t, MtlNodeType::Socket(_))), scan.cpu.socket_topology.len());
        assert_eq!(count(&g, |t| matches!(t, MtlNodeType::NumaDomain(_))), scan.numa_nodes.len());
        let expected_edges: usize = scan.cpu.socket_topology.iter().map(|s| s.cpus.len()).sum::<usize>()
            + scan.numa_nodes.iter().map(|n| n.cpus.len()).sum::<usize>();
        assert_eq!(g.edges.len(), expected_edges);
        assert_eq!(count(&g, |t| *t == MtlNodeType::GpuAccelerator), 0);
    }

    #[test]
    fn old_or_unknown_schema_is_an_error() {
        let mut v: serde_json::Value = serde_json::from_str(LINUX).unwrap();
        v["schema_version"] = serde_json::json!(2);
        let err = MapMtlBridge::parse_scan(v.to_string().as_bytes()).unwrap_err();
        assert!(err.contains("schema_version 2"), "{err}");
        let old = r#"{"nodes":[],"edges":[]}"#;
        assert!(MapMtlBridge::parse_scan(old.as_bytes()).unwrap_err().contains("unreadable"));
    }

    #[test]
    fn missing_binary_is_an_error_not_a_made_up_topology() {
        // Only this test touches LTN_MAP_MTL_BIN in this crate.
        std::env::set_var("LTN_MAP_MTL_BIN", "ltn-definitely-missing-map-mtl-binary");
        let r = MapMtlBridge::discover_topology();
        std::env::remove_var("LTN_MAP_MTL_BIN");
        let err = r.unwrap_err();
        assert!(err.contains("could not run"), "{err}");
    }
}
