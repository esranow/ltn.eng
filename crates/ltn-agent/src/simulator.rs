use ltn_architect::classifier::WorkloadClassifier;
use ltn_architect::parsers::ManifestParser;
use ltn_architect::planner::ai_cilium::{AiCiliumPlanner, ZeroTrustNetworkContract};
use ltn_core::ir::{
    Architecture, ComputeRequirements, NetworkRequirements, PlacementConstraints, PortDefinition,
    ProviderTarget, ReplicaPolicy, StorageRequirements, WorkloadDefinition, WorkloadKind,
};
use ltn_core::solver::{SimulationProofCertificate, SmtSolver, SolverStatus};
use ltn_core::topology::{HostHardwareInventory, SimulatedHardwarePlacement};
use ltn_core::vault::{SimulatedVaultReport, ZeroTrustVault};
use ltn_ops::canary::{CanaryOrchestrator, SimulatedRolloutReport};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeterministicSimulationReport {
    pub target_path: String,
    pub detected_manifests: Vec<String>,
    pub workload_classification: String,
    pub smt_proof_certificate: SimulationProofCertificate,
    pub hardware_placement: SimulatedHardwarePlacement,
    pub network_contract: ZeroTrustNetworkContract,
    pub vault_report: SimulatedVaultReport,
    pub canary_report: SimulatedRolloutReport,
    pub overall_sat: bool,
    pub simulation_hash: String,
    pub total_duration_ms: f64,
}

pub struct DeterministicSimulator;

impl DeterministicSimulator {
    /// Computes a deterministic SHA-256 simulation hash across target, solver status,
    /// proof hash, and canary rollout results.
    pub fn compute_simulation_hash(
        target_path: &str,
        solver_status: &SolverStatus,
        proof_hash: &str,
        canary_report: &SimulatedRolloutReport,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(target_path.as_bytes());
        hasher.update(b"|");
        hasher.update(format!("{:?}", solver_status).as_bytes());
        hasher.update(b"|");
        hasher.update(proof_hash.as_bytes());
        hasher.update(b"|");
        hasher.update(canary_report.overall_success.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(canary_report.final_traffic_pct.to_string().as_bytes());
        hasher.update(b"|");
        hasher.update(canary_report.rollback_triggered.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Resolves target path, checking direct path first and falling back to repository root if needed.
    fn resolve_target_path(target_path: &str) -> PathBuf {
        let direct = Path::new(target_path);
        if direct.exists() {
            return direct.to_path_buf();
        }

        // Check relative to workspace/repository root
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Some(workspace_root) = manifest_dir.parent().and_then(|p| p.parent()) {
            let candidate = workspace_root.join(target_path);
            if candidate.exists() {
                return candidate;
            }
        }

        direct.to_path_buf()
    }

    /// Orchestrates all simulation stages deterministically:
    /// 1. Parse target manifests with `ManifestParser::discover_target`
    /// 2. Classify workload with `WorkloadClassifier::classify`
    /// 3. Build `Architecture` IR representing the workload
    /// 4. Probe local host hardware inventory
    /// 5. Generate formal SMT certificate via `SmtSolver::generate_simulation_certificate`
    /// 6. Compute simulated hardware placement via `inventory.simulate_placement`
    /// 7. Synthesize zero-trust network contract via `AiCiliumPlanner::synthesize_network_contract`
    /// 8. Simulate zero-trust vault transaction via `ZeroTrustVault::simulate_vault_transaction`
    /// 9. Run staged canary rollout simulation via `CanaryOrchestrator::simulate_rollout(None)`
    /// 10. Compute deterministic SHA-256 simulation hash and elapsed duration
    pub fn simulate(target_path: &str) -> Result<DeterministicSimulationReport, String> {
        let start = std::time::Instant::now();
        let resolved_path = Self::resolve_target_path(target_path);

        // 1. Discover and parse target manifests
        let discovery = ManifestParser::discover_target(&resolved_path);
        let detected_manifests = discovery.detected_files.clone();

        // 2. Classify workload
        let classification = WorkloadClassifier::classify(&discovery, target_path);
        let workload_classification = classification.description.clone();

        // 3. Build Architecture IR representing the workload
        let service_name = resolved_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("simulated-service")
            .to_string();

        let port: u16 = match classification.workload_kind {
            WorkloadKind::StaticWeb => 80,
            WorkloadKind::NodePostgresApi => 3000,
            WorkloadKind::PythonVllmGpu => 8000,
            WorkloadKind::StatefulStore => 5432,
            WorkloadKind::GoWorker => 9090,
            WorkloadKind::RustWasm => 8080,
            WorkloadKind::GenericContainer => 8080,
        };

        let mut arch = Architecture::new(&service_name, ProviderTarget::LocalDocker);
        let workload_def = WorkloadDefinition {
            id: Uuid::new_v4(),
            name: service_name.clone(),
            kind: classification.workload_kind.clone(),
            container_image: format!("{}:latest", service_name),
            entrypoint: None,
            ports: vec![PortDefinition {
                container_port: port,
                host_port: Some(port),
                protocol: "TCP".to_string(),
            }],
            secrets: vec![],
            compute: ComputeRequirements {
                cpu_cores: classification.recommended_cpu_cores,
                ram_mb: classification.recommended_ram_mb,
                gpu_count: classification.recommended_gpu_count,
                vram_mb: classification.recommended_vram_mb,
                gpu_model_preference: None,
            },
            storage: StorageRequirements::default(),
            network: NetworkRequirements {
                ingress_public: true,
                required_bandwidth_gbps: 1,
                service_mesh_wireguard: true,
                allowed_cidr_blocks: vec!["0.0.0.0/0".to_string()],
            },
            replicas: ReplicaPolicy::default(),
            placement: PlacementConstraints::default(),
        };
        arch.workloads.push(workload_def);

        // 4. Probe local hardware inventory
        let inventory = HostHardwareInventory::probe_local_system();

        // 5. Generate formal SMT certificate
        let smt_proof_certificate = SmtSolver::generate_simulation_certificate(&arch, &inventory);

        // 6. Compute simulated hardware placement
        let hardware_placement = inventory.simulate_placement(
            classification.recommended_cpu_cores,
            classification.recommended_ram_mb,
            classification.recommended_gpu_count,
        );

        // 7. Synthesize zero-trust network contract
        let egress_dependencies = match classification.workload_kind {
            WorkloadKind::NodePostgresApi => vec!["postgres:5432".to_string()],
            WorkloadKind::StatefulStore => vec!["backup-store:9000".to_string()],
            WorkloadKind::PythonVllmGpu => vec!["model-registry:8000".to_string()],
            WorkloadKind::GoWorker => vec!["broker:9092".to_string()],
            _ => vec![],
        };
        let network_contract =
            AiCiliumPlanner::synthesize_network_contract(&service_name, port, &egress_dependencies);

        // 8. Simulate zero-trust vault transaction
        let mut sample_secrets = HashMap::new();
        sample_secrets.insert(
            "DATABASE_URL".to_string(),
            "postgres://sim_user:sim_pass@127.0.0.1:5432/app_db".to_string(),
        );
        sample_secrets.insert(
            "SECRET_KEY".to_string(),
            "ltn_deterministic_sim_secret_token".to_string(),
        );
        let vault_report = ZeroTrustVault::simulate_vault_transaction(
            &sample_secrets,
            "ltn-deterministic-simulation-passphrase",
        )
        .map_err(|e| format!("Zero-trust vault simulation failed: {}", e))?;

        // 9. Run staged canary rollout simulation
        let mut canary_orchestrator = CanaryOrchestrator::new();
        let canary_report = canary_orchestrator.simulate_rollout(None);

        // 10. Compute overall SAT status
        let overall_sat =
            smt_proof_certificate.status == SolverStatus::SAT && canary_report.overall_success;

        // 11. Compute deterministic SHA-256 simulation hash across target, solver status, proof hash, and canary results
        let simulation_hash = Self::compute_simulation_hash(
            target_path,
            &smt_proof_certificate.status,
            &smt_proof_certificate.proof_hash,
            &canary_report,
        );

        // 12. Measure total elapsed duration in ms
        let total_duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        Ok(DeterministicSimulationReport {
            target_path: target_path.to_string(),
            detected_manifests,
            workload_classification,
            smt_proof_certificate,
            hardware_placement,
            network_contract,
            vault_report,
            canary_report,
            overall_sat,
            simulation_hash,
            total_duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_simulator_node_postgres_api() {
        let fixture_path = "tests/fixtures/node_postgres_api";
        let res = DeterministicSimulator::simulate(fixture_path);
        assert!(
            res.is_ok(),
            "Simulator must succeed on valid fixture: {:?}",
            res.err()
        );

        let report = res.unwrap();
        assert_eq!(report.target_path, fixture_path);
        assert!(
            report
                .detected_manifests
                .contains(&"package.json".to_string()),
            "Should discover package.json in fixture, detected: {:?}",
            report.detected_manifests
        );
        assert!(
            report.workload_classification.contains("Node.js")
                || report.workload_classification.contains("API")
                || report.workload_classification.contains("REST"),
            "Classification should identify Node.js workload: {}",
            report.workload_classification
        );

        // SMT Certificate verification
        assert!(!report.smt_proof_certificate.proof_hash.is_empty());
        assert!(report.smt_proof_certificate.anti_autoscaling_headroom_ratio > 1.0);

        // Hardware Placement verification
        assert!(!report.hardware_placement.target_node_name.is_empty());
        assert!(!report.hardware_placement.pinned_cpu_cores.is_empty());
        assert_eq!(report.hardware_placement.cpu_governor, "performance");

        // Zero-Trust Network Contract verification
        assert!(!report.network_contract.ingress_rules.is_empty());
        assert!(!report.network_contract.wireguard_mesh_peer.is_empty());
        assert!(!report.network_contract.nftables_ruleset.is_empty());
        assert!(!report.network_contract.ebpf_filter_bytecode_spec.is_empty());

        // Zero-Trust Vault Report verification
        assert!(report.vault_report.encryption_successful);
        assert!(report.vault_report.integrity_verified);
        assert!(report.vault_report.zeroization_verified);
        assert_eq!(report.vault_report.keys_managed.len(), 2);

        // Canary Rollout Report verification
        assert!(report.canary_report.overall_success);
        assert_eq!(report.canary_report.final_traffic_pct, 100);
        assert!(!report.canary_report.rollback_triggered);
        assert_eq!(report.canary_report.stages.len(), 4);

        // Deterministic Hash and Duration verification
        assert_eq!(report.simulation_hash.len(), 64);
        assert!(report.total_duration_ms > 0.0);
    }

    #[test]
    fn test_deterministic_simulator_python_vllm_gpu() {
        let fixture_path = "tests/fixtures/python_vllm_gpu";
        let res = DeterministicSimulator::simulate(fixture_path);
        assert!(
            res.is_ok(),
            "Simulator must succeed on Python GPU fixture: {:?}",
            res.err()
        );

        let report = res.unwrap();
        assert_eq!(report.target_path, fixture_path);
        assert!(
            report
                .detected_manifests
                .contains(&"pyproject.toml".to_string()),
            "Should discover pyproject.toml in fixture, detected: {:?}",
            report.detected_manifests
        );
        assert_eq!(report.simulation_hash.len(), 64);
        assert!(report.vault_report.integrity_verified);
        assert!(report.canary_report.overall_success);
    }

    #[test]
    fn test_deterministic_simulator_hash_function_determinism() {
        let dummy_canary = SimulatedRolloutReport {
            stages: vec![],
            overall_success: true,
            final_traffic_pct: 100,
            rollback_triggered: false,
            rollback_latency_ms: 0.0,
            slo_budget_burn_rate: 0.2,
        };

        let hash1 = DeterministicSimulator::compute_simulation_hash(
            "/workspace/service-a",
            &SolverStatus::SAT,
            "abc123proofhash",
            &dummy_canary,
        );

        let hash2 = DeterministicSimulator::compute_simulation_hash(
            "/workspace/service-a",
            &SolverStatus::SAT,
            "abc123proofhash",
            &dummy_canary,
        );

        assert_eq!(
            hash1, hash2,
            "Identical inputs must yield identical SHA-256 hashes"
        );
        assert_eq!(hash1.len(), 64);

        // Modifying any parameter must change the simulation hash (avalanche effect)
        let hash3 = DeterministicSimulator::compute_simulation_hash(
            "/workspace/service-b",
            &SolverStatus::SAT,
            "abc123proofhash",
            &dummy_canary,
        );
        assert_ne!(
            hash1, hash3,
            "Different target path must yield different hash"
        );

        let hash4 = DeterministicSimulator::compute_simulation_hash(
            "/workspace/service-a",
            &SolverStatus::UNSAT,
            "abc123proofhash",
            &dummy_canary,
        );
        assert_ne!(
            hash1, hash4,
            "Different solver status must yield different hash"
        );
    }

    #[test]
    fn test_deterministic_simulation_report_serde_roundtrip() {
        let res = DeterministicSimulator::simulate("tests/fixtures/node_postgres_api").unwrap();
        let json = serde_json::to_string(&res).expect("Serialization to JSON must succeed");
        let deserialized: DeterministicSimulationReport =
            serde_json::from_str(&json).expect("Deserialization from JSON must succeed");

        assert_eq!(res.target_path, deserialized.target_path);
        assert_eq!(res.detected_manifests, deserialized.detected_manifests);
        assert_eq!(
            res.workload_classification,
            deserialized.workload_classification
        );
        assert_eq!(
            res.smt_proof_certificate,
            deserialized.smt_proof_certificate
        );
        assert_eq!(res.hardware_placement, deserialized.hardware_placement);
        assert_eq!(res.network_contract, deserialized.network_contract);
        assert_eq!(res.vault_report, deserialized.vault_report);
        assert_eq!(res.canary_report, deserialized.canary_report);
        assert_eq!(res.overall_sat, deserialized.overall_sat);
        assert_eq!(res.simulation_hash, deserialized.simulation_hash);
        assert!((res.total_duration_ms - deserialized.total_duration_ms).abs() < 1e-3);
    }
}
