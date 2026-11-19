use crate::observer::SystemObservation;
use ltn_core::guardrails::{CapabilityContext, PolicyEngine};
use ltn_core::ir::Architecture;
use ltn_core::solver::SmtSolver;
use ltn_core::topology::HostHardwareInventory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FailureClassification {
    OomKilled,
    GpuMemoryExhaustion,
    P99LatencyViolation,
    DatabaseSaturation,
    NetworkSaturation,
    HealthCheckTimeout,
    PortCollision,
    GenericFailure(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    pub metric_name: String,
    pub observed_value: String,
    pub threshold: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchitectureDelta {
    pub ram_scale_factor: Option<f32>,
    pub replica_delta: Option<i32>,
    pub enable_caching: bool,
    pub quantization_change: Option<String>,
    pub bandwidth_boost_gbps: Option<u32>,
    pub notes: String,
}

impl ArchitectureDelta {
    pub fn apply(&self, arch: &mut Architecture) {
        for w in &mut arch.workloads {
            if let Some(scale) = self.ram_scale_factor {
                w.compute.ram_mb = ((w.compute.ram_mb as f32) * scale).round() as u32;
            }
            if let Some(delta) = self.replica_delta {
                let new_desired = (w.replicas.desired_replicas as i32 + delta).max(1) as u32;
                w.replicas.desired_replicas = new_desired;
                w.replicas.max_replicas = w.replicas.max_replicas.max(new_desired + 2);
            }
            if let Some(ref quant) = self.quantization_change {
                if let Some(ref pref) = w.compute.gpu_model_preference {
                    w.compute.gpu_model_preference =
                        Some(format!("{} (Quantized: {})", pref, quant));
                } else {
                    w.compute.gpu_model_preference = Some(format!("Quantized: {}", quant));
                }
                // Quantization significantly reduces VRAM requirement
                w.compute.vram_mb = (w.compute.vram_mb / 2).max(4096);
            }
            if let Some(bw) = self.bandwidth_boost_gbps {
                w.network.required_bandwidth_gbps = w.network.required_bandwidth_gbps.max(bw);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosisReport {
    pub failure_class: FailureClassification,
    pub root_cause: String,
    pub evidence: Vec<Evidence>,
    pub delta: ArchitectureDelta,
    pub revalidation_sat: bool,
    pub policy_approved: bool,
    pub timestamp: String,
}

pub struct AutonomousDiagnosisEngine;

impl AutonomousDiagnosisEngine {
    /// Autonomous diagnosis and replanning pipeline:
    /// Observed Telemetry -> Failure Classification -> Evidence Gathering -> Diagnosis -> Architecture Delta -> Revalidation
    pub fn diagnose_and_replan(
        observation: &SystemObservation,
        current_arch: &Architecture,
        hardware: &HostHardwareInventory,
        context: &CapabilityContext,
    ) -> (DiagnosisReport, Architecture) {
        let mut evidence = Vec::new();
        let (failure_class, root_cause, delta) = if observation.memory_utilization_pct > 90.0 {
            evidence.push(Evidence {
                metric_name: "memory_utilization_pct".to_string(),
                observed_value: format!("{:.1}%", observation.memory_utilization_pct),
                threshold: "< 85.0%".to_string(),
            });
            (
                FailureClassification::OomKilled,
                "Process terminated or degraded due to host/container memory exhaustion (OOM).".to_string(),
                ArchitectureDelta {
                    ram_scale_factor: Some(1.5),
                    replica_delta: Some(1),
                    enable_caching: false,
                    quantization_change: None,
                    bandwidth_boost_gbps: None,
                    notes: "Scale RAM allocation by 1.5x and add 1 replica to redistribute memory load.".to_string(),
                },
            )
        } else if observation.latency.p99_ms > current_arch.availability.max_p99_latency_ms {
            evidence.push(Evidence {
                metric_name: "latency.p99_ms".to_string(),
                observed_value: format!("{:.1}ms", observation.latency.p99_ms),
                threshold: format!("< {:.1}ms", current_arch.availability.max_p99_latency_ms),
            });
            (
                FailureClassification::P99LatencyViolation,
                format!("p99 tail latency ({:.1}ms) breached declared SLO budget ({:.1}ms).", observation.latency.p99_ms, current_arch.availability.max_p99_latency_ms),
                ArchitectureDelta {
                    ram_scale_factor: None,
                    replica_delta: Some(2),
                    enable_caching: true,
                    quantization_change: None,
                    bandwidth_boost_gbps: Some(5),
                    notes: "Scale out replicas (+2), enable Redis caching layer, and allocate 5Gbps network bandwidth.".to_string(),
                },
            )
        } else if observation.vram_utilization_pct.unwrap_or(0.0) > 95.0 {
            evidence.push(Evidence {
                metric_name: "vram_utilization_pct".to_string(),
                observed_value: format!("{:.1}%", observation.vram_utilization_pct.unwrap_or(0.0)),
                threshold: "< 90.0%".to_string(),
            });
            (
                FailureClassification::GpuMemoryExhaustion,
                "GPU VRAM allocation exhausted during model batch inferencing.".to_string(),
                ArchitectureDelta {
                    ram_scale_factor: None,
                    replica_delta: None,
                    enable_caching: false,
                    quantization_change: Some("AWQ_4BIT".to_string()),
                    bandwidth_boost_gbps: None,
                    notes: "Apply 4-bit AWQ quantization to reduce model VRAM requirement by 50%."
                        .to_string(),
                },
            )
        } else if observation.error_rate_pct > 0.05 {
            evidence.push(Evidence {
                metric_name: "error_rate_pct".to_string(),
                observed_value: format!("{:.4}%", observation.error_rate_pct * 100.0),
                threshold: "< 0.05%".to_string(),
            });
            (
                FailureClassification::DatabaseSaturation,
                "Elevated 5xx error rate indicating upstream service or database connection saturation.".to_string(),
                ArchitectureDelta {
                    ram_scale_factor: Some(1.25),
                    replica_delta: Some(1),
                    enable_caching: true,
                    quantization_change: None,
                    bandwidth_boost_gbps: None,
                    notes: "Deploy connection pooler buffer, add read replica, and increase RAM.".to_string(),
                },
            )
        } else {
            evidence.push(Evidence {
                metric_name: "health_status".to_string(),
                observed_value: observation.health_status.clone(),
                threshold: "HEALTHY".to_string(),
            });
            (
                FailureClassification::HealthCheckTimeout,
                "Health check probe failed or timed out.".to_string(),
                ArchitectureDelta {
                    ram_scale_factor: Some(1.2),
                    replica_delta: None,
                    enable_caching: false,
                    quantization_change: None,
                    bandwidth_boost_gbps: None,
                    notes: "Increase process startup headroom and adjust health probe timeouts."
                        .to_string(),
                },
            )
        };

        // Apply proposed delta to generate updated Architecture
        let mut replanned_arch = current_arch.clone();
        replanned_arch.metadata.version = format!(
            "{}-replan-{}",
            current_arch.metadata.version,
            chrono::Utc::now().timestamp()
        );
        delta.apply(&mut replanned_arch);

        // Revalidate against Z3 SMT solver and Policy Engine
        let validation_res = SmtSolver::validate_architecture(&replanned_arch, hardware);
        let policy_res =
            PolicyEngine::evaluate_architecture_and_context(&replanned_arch, context, true, false);

        let report = DiagnosisReport {
            failure_class,
            root_cause,
            evidence,
            delta,
            revalidation_sat: validation_res.status == ltn_core::solver::SolverStatus::SAT,
            policy_approved: policy_res.approved,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        (report, replanned_arch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::LatencyPercentiles;
    use ltn_core::ir::*;
    use uuid::Uuid;

    #[test]
    fn test_diagnosis_p99_latency_spike_replan() {
        let mut arch = Architecture::new("api-service", ProviderTarget::AWS);
        arch.availability.max_p99_latency_ms = 100.0;
        arch.workloads.push(WorkloadDefinition {
            id: Uuid::new_v4(),
            name: "api".to_string(),
            kind: WorkloadKind::NodePostgresApi,
            container_image: "api:v1".to_string(),
            entrypoint: None,
            ports: vec![],
            secrets: vec![],
            compute: ComputeRequirements {
                cpu_cores: 1,
                ram_mb: 512,
                gpu_count: 0,
                vram_mb: 0,
                gpu_model_preference: None,
            },
            storage: StorageRequirements::default(),
            network: NetworkRequirements::default(),
            replicas: ReplicaPolicy {
                min_replicas: 1,
                max_replicas: 3,
                desired_replicas: 1,
            },
            placement: PlacementConstraints::default(),
        });

        // Simulate p99 latency spike to 350ms (> 100ms SLO)
        let obs = SystemObservation {
            timestamp: "now".to_string(),
            workload_id: "api".to_string(),
            cpu_utilization_pct: 75.0,
            memory_utilization_pct: 60.0,
            memory_used_mb: 300,
            gpu_utilization_pct: None,
            vram_utilization_pct: None,
            vram_used_mb: None,
            network_rx_mbps: 10.0,
            network_tx_mbps: 10.0,
            storage_used_gb: 1.0,
            request_rate_qps: 500.0,
            error_rate_pct: 0.0001,
            latency: LatencyPercentiles {
                p50_ms: 50.0,
                p95_ms: 220.0,
                p99_ms: 350.0,
            },
            health_status: "DEGRADED".to_string(),
            provider_metadata: std::collections::HashMap::new(),
        };

        let hw = HostHardwareInventory::default();
        let ctx = CapabilityContext::default();
        let (report, new_arch) =
            AutonomousDiagnosisEngine::diagnose_and_replan(&obs, &arch, &hw, &ctx);

        assert_eq!(
            report.failure_class,
            FailureClassification::P99LatencyViolation
        );
        assert_eq!(new_arch.workloads[0].replicas.desired_replicas, 3); // 1 + 2 = 3
        assert!(report.revalidation_sat);
        assert!(report.policy_approved);
    }
}
