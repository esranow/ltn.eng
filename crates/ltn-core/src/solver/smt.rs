use crate::ir::{ClusterState, Constraint, Intent, ResourceBlock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum SolverError {
    #[error("Unsatisfiable constraints detected: {mus_core:?}")]
    Unsatisfiable { mus_core: Vec<String> },

    #[error("Cycle detected in workload dependency graph")]
    CycleDetected,

    #[error("Internal solver failure: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecManifest {
    pub intent_id: Uuid,
    pub mappings: HashMap<Uuid, Uuid>,
    pub total_cost_cents: u64,
}

pub struct SmtSolver;

impl SmtSolver {
    pub fn solve(intent: &Intent, cluster: &ClusterState) -> Result<ExecManifest, SolverError> {
        let mut mappings = HashMap::new();
        let mut mus_core = Vec::new();

        for workload in &intent.workloads {
            let mut matched_block: Option<&ResourceBlock> = None;
            let mut workload_failures = Vec::new();

            for block in &cluster.resources {
                let mut valid = true;
                for constraint in &workload.constraints {
                    match constraint {
                        Constraint::CpuCores(cores) => {
                            if block.total_cpu_cores < *cores {
                                valid = false;
                                workload_failures.push(format!(
                                    "Workload '{}' requires CPU cores {}, block '{}' has {}",
                                    workload.name, cores, block.node_name, block.total_cpu_cores
                                ));
                            }
                        }
                        Constraint::RamMb(ram) => {
                            if block.total_ram_mb < *ram {
                                valid = false;
                                workload_failures.push(format!(
                                    "Workload '{}' requires RAM {}MB, block '{}' has {}MB",
                                    workload.name, ram, block.node_name, block.total_ram_mb
                                ));
                            }
                        }
                        Constraint::VramMb(vram) => {
                            if block.total_vram_mb < *vram {
                                valid = false;
                                workload_failures.push(format!(
                                    "Workload '{}' requires VRAM {}MB, block '{}' has {}MB",
                                    workload.name, vram, block.node_name, block.total_vram_mb
                                ));
                            }
                        }
                        Constraint::GpuCount(gpus) => {
                            if block.total_gpu_count < *gpus {
                                valid = false;
                                workload_failures.push(format!(
                                    "Workload '{}' requires GPUs {}, block '{}' has {}",
                                    workload.name, gpus, block.node_name, block.total_gpu_count
                                ));
                            }
                        }
                        Constraint::NetworkGbps(gbps) if block.network_bw_gbps < *gbps => {
                            valid = false;
                            workload_failures.push(format!(
                                "Workload '{}' requires Network {}Gbps, block '{}' has {}Gbps",
                                workload.name, gbps, block.node_name, block.network_bw_gbps
                            ));
                        }
                        _ => {}
                    }
                }

                if valid {
                    matched_block = Some(block);
                    break;
                }
            }

            if let Some(block) = matched_block {
                mappings.insert(workload.id, block.block_id);
            } else {
                mus_core.extend(workload_failures);
                mus_core.push(format!("No resource block in cluster satisfies workload '{}'", workload.name));
            }
        }

        if !mus_core.is_empty() {
            return Err(SolverError::Unsatisfiable { mus_core });
        }

        Ok(ExecManifest {
            intent_id: intent.intent_id,
            mappings,
            total_cost_cents: 100,
        })
    }
}
