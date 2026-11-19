use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedParallelismSpec {
    pub framework: String, // "PyTorch" | "DeepSpeed" | "vLLM" | "Megatron"
    pub tensor_parallel_size: u32,
    pub pipeline_parallel_size: u32,
    pub data_parallel_size: u32,
    pub world_size: u32,
    pub master_port: u16,
    pub gpu_nvlink_pinned_devices: Vec<u32>,
    pub numa_node_binding: Vec<u32>,
    pub launch_command: String,
}

pub struct AiSlinkyPlanner;

impl AiSlinkyPlanner {
    /// Discovers distributed training intent and synthesizes rank-aware gang placement without Slurm/Kubernetes
    pub fn plan_gpu_workload(
        project_path: &Path,
        available_gpus: u32,
        _vram_per_gpu_mb: u64,
    ) -> DistributedParallelismSpec {
        let is_vllm = project_path.join("vllm.yaml").exists()
            || project_path.join("serve.py").exists()
            || project_path.to_string_lossy().contains("vllm");

        let (tp, pp, dp, framework) = if is_vllm {
            // For vLLM inference, TP typically equals min(available_gpus, 8), PP=1, DP=1
            let tp = if available_gpus >= 2 { 2 } else { 1 };
            (tp, 1, 1, "vLLM-OpenAI".to_string())
        } else if available_gpus >= 4 {
            (2, 2, 1, "DeepSpeed-ZeRO3".to_string())
        } else if available_gpus >= 2 {
            (2, 1, 1, "PyTorch-DDP".to_string())
        } else {
            (1, 1, 1, "PyTorch-Single".to_string())
        };

        let world_size = tp * pp * dp;
        let pinned_devices: Vec<u32> = (0..world_size.min(available_gpus)).collect();
        let numa_nodes: Vec<u32> = pinned_devices.iter().map(|d| d / 4).collect();

        let devices_str = pinned_devices
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let launch_cmd = match framework.as_str() {
            "vLLM-OpenAI" => {
                format!(
                    "numactl --cpunodebind=0 --membind=0 vllm serve --tensor-parallel-size {} --gpu-memory-utilization 0.90",
                    tp
                )
            }
            "DeepSpeed-ZeRO3" => {
                format!(
                    "CUDA_VISIBLE_DEVICES={} deepspeed --num_gpus={} train.py",
                    devices_str, world_size
                )
            }
            _ => {
                format!(
                    "CUDA_VISIBLE_DEVICES={} torchrun --nproc_per_node={} train.py",
                    devices_str, world_size
                )
            }
        };

        DistributedParallelismSpec {
            framework,
            tensor_parallel_size: tp,
            pipeline_parallel_size: pp,
            data_parallel_size: dp,
            world_size,
            master_port: 29500,
            gpu_nvlink_pinned_devices: pinned_devices,
            numa_node_binding: numa_nodes,
            launch_command: launch_cmd,
        }
    }
}
