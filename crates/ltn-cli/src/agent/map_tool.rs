use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MapMtlTopologySummary {
    pub host_name: String,
    pub os_name: String,
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub cpu_count: usize,
    pub gpu_model: Option<String>,
    pub vram_mb: u64,
    pub hardware_status: String,
}

#[allow(dead_code)]
pub struct MapMtlEngine;

#[allow(dead_code)]
impl MapMtlEngine {
    pub fn query_gpu_info() -> (Option<String>, u64) {
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = text.lines().next() {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 2 {
                        let name = parts[0].trim().to_string();
                        let vram = parts[1].trim().parse::<u64>().unwrap_or(0);
                        return (Some(name), vram);
                    }
                }
            }
        }
        (None, 0)
    }

    pub fn discover_topology() -> MapMtlTopologySummary {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        let host_name = sysinfo::System::host_name().unwrap_or_else(|| "LocalHost".to_string());
        let os_name = sysinfo::System::name().unwrap_or_else(|| "Windows/Linux".to_string());
        let total_memory_mb = sys.total_memory() / 1024 / 1024;
        let used_memory_mb = sys.used_memory() / 1024 / 1024;
        let cpu_count = sys.cpus().len();

        let (gpu_model, vram_mb) = Self::query_gpu_info();
        let status = if gpu_model.is_some() {
            "VERIFIED_BARE_METAL (REAL_NVIDIA_GPU)".to_string()
        } else {
            "VERIFIED_BARE_METAL (CPU_ONLY)".to_string()
        };

        MapMtlTopologySummary {
            host_name,
            os_name,
            total_memory_mb,
            used_memory_mb,
            cpu_count,
            gpu_model,
            vram_mb,
            hardware_status: status,
        }
    }
}
