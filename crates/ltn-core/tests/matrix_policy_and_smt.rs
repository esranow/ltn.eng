use ltn_core::guardrails::{AgentCapability, CapabilityContext, PolicyEngine, PolicyViolationType};
use ltn_core::ir::{
    Architecture, ComputeRequirements, NetworkRequirements, PlacementConstraints, PortDefinition,
    ProviderTarget, ReplicaPolicy, SecretReference, StorageRequirements, WorkloadDefinition,
    WorkloadKind,
};
use ltn_core::solver::{SmtSolver, SolverStatus};
use ltn_core::state_machine::{MutationState, StateMachine};
use ltn_core::topology::HostHardwareInventory;
use uuid::Uuid;

// =========================================================================
// LAYER 1: Canonical IR Determinism, Hashing, and SMT Bin-Packing Tests
// =========================================================================

#[test]
fn test_canonical_ir_hashing_determinism() {
    let mut arch1 = Architecture::new("service-alpha", ProviderTarget::AWS);
    arch1.workloads.push(WorkloadDefinition {
        id: Uuid::from_u128(1),
        name: "api".to_string(),
        kind: WorkloadKind::NodePostgresApi,
        container_image: "node:18".to_string(),
        entrypoint: None,
        ports: vec![PortDefinition {
            container_port: 8080,
            host_port: Some(8080),
            protocol: "TCP".to_string(),
        }],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 2,
            ram_mb: 2048,
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

    let mut arch2 = Architecture::new("service-alpha", ProviderTarget::AWS);
    arch2.metadata = arch1.metadata.clone();
    arch2.workloads.push(WorkloadDefinition {
        id: Uuid::from_u128(1),
        name: "api".to_string(),
        kind: WorkloadKind::NodePostgresApi,
        container_image: "node:18".to_string(),
        entrypoint: None,
        ports: vec![PortDefinition {
            container_port: 8080,
            host_port: Some(8080),
            protocol: "TCP".to_string(),
        }],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 2,
            ram_mb: 2048,
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

    // Identical canonical structures must produce strictly identical cryptographic hashes
    assert_eq!(arch1.canonical_hash(), arch2.canonical_hash());

    // Modify a field -> hash MUST change
    arch2.workloads[0].compute.ram_mb = 4096;
    assert_ne!(arch1.canonical_hash(), arch2.canonical_hash());
}

#[test]
fn test_canonical_diff_generation() {
    let mut arch1 = Architecture::new("service-alpha", ProviderTarget::AWS);
    arch1.workloads.push(WorkloadDefinition {
        id: Uuid::from_u128(1),
        name: "api".to_string(),
        kind: WorkloadKind::NodePostgresApi,
        container_image: "api:v1".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 2,
            ram_mb: 2048,
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

    let mut arch2 = arch1.clone();
    arch2.workloads[0].compute.ram_mb = 4096; // modified
    arch2.workloads.push(WorkloadDefinition {
        id: Uuid::from_u128(2),
        name: "cache".to_string(),
        kind: WorkloadKind::GenericContainer,
        container_image: "redis:alpine".to_string(),
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
            max_replicas: 1,
            desired_replicas: 1,
        },
        placement: PlacementConstraints::default(),
    });

    let diff = arch1.diff(&arch2);
    assert_eq!(diff.workload_additions.len(), 1);
    assert_eq!(diff.workload_additions[0].name, "cache");
    assert_eq!(diff.workload_modifications.len(), 1);
    assert_eq!(diff.workload_removals.len(), 0);
    assert!(!diff.hash_matches);
}

#[test]
fn test_structured_z3_validation_sat_for_feasible_workload() {
    let mut arch = Architecture::new("api", ProviderTarget::LocalDocker);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "web".to_string(),
        kind: WorkloadKind::NodePostgresApi,
        container_image: "web:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 2,
            ram_mb: 2048,
            gpu_count: 0,
            vram_mb: 0,
            gpu_model_preference: None,
        },
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 2,
            desired_replicas: 1,
        },
        placement: PlacementConstraints::default(),
    });

    let mut hw = HostHardwareInventory::default();
    hw.cpu.logical_threads = 8;
    hw.ram.available_mb = 16384;

    let res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(res.status, SolverStatus::SAT);
    assert!(res.violations.is_empty());
}

#[test]
fn test_structured_z3_validation_unsat_insufficient_ram() {
    let mut arch = Architecture::new("high-mem-db", ProviderTarget::LocalDocker);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "db".to_string(),
        kind: WorkloadKind::StatefulStore,
        container_image: "postgres:16".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 2,
            ram_mb: 65536,
            gpu_count: 0,
            vram_mb: 0,
            gpu_model_preference: None,
        }, // 64 GB
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 2,
            desired_replicas: 1,
        },
        placement: PlacementConstraints::default(),
    });

    let mut hw = HostHardwareInventory::default();
    hw.ram.available_mb = 8192; // Only 8 GB available

    let res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(res.status, SolverStatus::UNSAT);
    assert!(!res.violations.is_empty());
    assert!(res
        .violations
        .iter()
        .any(|v| v.constraint_id.contains("RAM")));
    assert!(!res.violations[0].suggested_replanning_options.is_empty());
}

#[test]
fn test_structured_z3_validation_unsat_insufficient_vram_and_gpu() {
    let mut arch = Architecture::new("vllm-service", ProviderTarget::AWS);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "llm-inference".to_string(),
        kind: WorkloadKind::PythonVllmGpu,
        container_image: "vllm:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 4,
            ram_mb: 16384,
            gpu_count: 2,
            vram_mb: 49152,
            gpu_model_preference: Some("A100".to_string()),
        }, // 48GB VRAM
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 2,
            desired_replicas: 1,
        },
        placement: PlacementConstraints::default(),
    });

    let mut hw = HostHardwareInventory::default();
    hw.gpu.count = 0;
    hw.gpu.total_vram_mb = 0;

    let res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(res.status, SolverStatus::UNSAT);
    assert!(res
        .violations
        .iter()
        .any(|v| v.constraint_id.contains("GPU") || v.constraint_id.contains("VRAM")));
}

#[test]
fn test_structured_z3_validation_unsat_insufficient_cpu_cores() {
    let mut arch = Architecture::new("heavy-compute", ProviderTarget::LocalDocker);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "worker".to_string(),
        kind: WorkloadKind::GoWorker,
        container_image: "worker:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 64,
            ram_mb: 4096,
            gpu_count: 0,
            vram_mb: 0,
            gpu_model_preference: None,
        }, // 64 cores
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 2,
            desired_replicas: 1,
        },
        placement: PlacementConstraints::default(),
    });

    let mut hw = HostHardwareInventory::default();
    hw.cpu.logical_threads = 8;

    let res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(res.status, SolverStatus::UNSAT);
    assert!(res
        .violations
        .iter()
        .any(|v| v.constraint_id.contains("CPU")));
}

// =========================================================================
// LAYER 2: Deterministic Policy Engine Tests (All 8 Rules Independently)
// =========================================================================

#[test]
fn test_policy_rule_1_plaintext_secret_violation() {
    let mut arch = Architecture::new("service", ProviderTarget::AWS);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "web".to_string(),
        kind: WorkloadKind::NodePostgresApi,
        container_image: "web:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![SecretReference {
            env_var_name: "DB_PASSWORD".to_string(),
            vault_key: "   ".to_string(), // Empty vault key reference
            optional: false,
        }],
        compute: ComputeRequirements::default(),
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy::default(),
        placement: PlacementConstraints::default(),
    });

    let ctx = CapabilityContext::default();
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::PlaintextSecretDetected));
}

#[test]
fn test_policy_rule_2_missing_pre_mutation_snapshot_violation() {
    let arch = Architecture::new("service", ProviderTarget::AWS);
    let ctx = CapabilityContext::default();
    // has_prior_snapshot = false
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, false, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::MissingSnapshot));
}

#[test]
fn test_policy_rule_3_unauthorized_agent_action_violation() {
    let arch = Architecture::new("service", ProviderTarget::AWS);
    // Context lacking DeployStaging / DeployProduction capability
    let ctx = CapabilityContext {
        granted_capabilities: vec![AgentCapability::ReadProject, AgentCapability::Architect],
        environment: "staging".to_string(),
        authorization_token: None,
    };
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::UnauthorizedMutation));
}

#[test]
fn test_policy_rule_4_budget_exceeded_violation() {
    let mut arch = Architecture::new("expensive-cluster", ProviderTarget::AWS);
    arch.cost_constraints.max_monthly_budget_usd = 100.0;
    // Workloads requiring massive compute exceeding budget
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "gpu-worker".to_string(),
        kind: WorkloadKind::PythonVllmGpu,
        container_image: "gpu:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements {
            cpu_cores: 32,
            ram_mb: 131072,
            gpu_count: 4,
            vram_mb: 98304,
            gpu_model_preference: None,
        },
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 4,
            desired_replicas: 2,
        },
        placement: PlacementConstraints::default(),
    });

    let ctx = CapabilityContext::default();
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::BudgetExceeded));
}

#[test]
fn test_policy_rule_5_supported_provider_check() {
    let arch = Architecture::new("supported-service", ProviderTarget::AWS);
    let ctx = CapabilityContext::default();
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(res.approved);
}

#[test]
fn test_policy_rule_6_quorum_failure_for_distributed_db() {
    let mut arch = Architecture::new("distributed-db", ProviderTarget::AWS);
    arch.workloads.push(WorkloadDefinition {
        id: Uuid::new_v4(),
        name: "cockroachdb".to_string(),
        kind: WorkloadKind::StatefulStore,
        container_image: "cockroachdb:latest".to_string(),
        entrypoint: None,
        ports: vec![],
        secrets: vec![],
        compute: ComputeRequirements::default(),
        storage: StorageRequirements::default(),
        network: NetworkRequirements::default(),
        // Replica count is only 2 (< 3 quorum requirement)
        replicas: ReplicaPolicy {
            min_replicas: 1,
            max_replicas: 2,
            desired_replicas: 2,
        },
        placement: PlacementConstraints::default(),
    });

    let ctx = CapabilityContext::default();
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::SinglePointOfFailure));
}

#[test]
fn test_policy_rule_7_mandatory_canary_for_production() {
    let mut arch = Architecture::new("prod-app", ProviderTarget::AWS);
    arch.availability.sla_target_pct = 99.95; // Production tier
    let ctx = CapabilityContext::default();
    // is_production = true
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, true);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::CanaryBypassed));
}

#[test]
fn test_policy_rule_8_slo_exceeds_physical_limits() {
    let mut arch = Architecture::new("wan-app", ProviderTarget::AWS);
    // Requesting sub-5ms p99 latency across multiple failure domains / WAN
    arch.availability.max_p99_latency_ms = 2.0;
    arch.failure_domains = vec!["us-east-1a".to_string(), "eu-west-1a".to_string()];

    let ctx = CapabilityContext::default();
    let res = PolicyEngine::evaluate_architecture_and_context(&arch, &ctx, true, false);
    assert!(!res.approved);
    assert!(res
        .violations
        .iter()
        .any(|v| v.rule == PolicyViolationType::SloViolation));
}

// =========================================================================
// LAYER 4: State Machine & Transaction Lifecycle Tests
// =========================================================================

#[test]
fn test_state_machine_full_forward_lifecycle() {
    let mut sm = StateMachine::new();
    let tx_id = sm.begin_mutation("deploy hash_abc to AWS");
    assert_eq!(tx_id, 1);
    assert_eq!(sm.mutations.len(), 1);

    // Forward progression: SNAPSHOTTED -> VALIDATED -> EXECUTED -> OBSERVED -> COMMITTED
    sm.advance_mutation(
        tx_id,
        MutationState::VALIDATED,
        "Z3 SMT Solver verified SAT",
    )
    .unwrap();
    sm.advance_mutation(
        tx_id,
        MutationState::EXECUTED,
        "Actuator provisioned resources",
    )
    .unwrap();
    sm.advance_mutation(
        tx_id,
        MutationState::OBSERVED,
        "Canary reached 100% with nominal metrics",
    )
    .unwrap();
    sm.advance_mutation(
        tx_id,
        MutationState::COMMITTED,
        "Autonomous deployment finalized",
    )
    .unwrap();

    let record = sm.get_mutation(tx_id).unwrap();
    assert_eq!(record.state, MutationState::COMMITTED);
    assert_eq!(record.details.len(), 5);
}

#[test]
fn test_state_machine_rollback_lifecycle() {
    let mut sm = StateMachine::new();
    sm.set_key("version", "v1.0.0");
    let snap = sm.commit();

    let tx_id = sm.begin_mutation("deploy candidate v2.0.0");
    sm.set_key("version", "v2.0.0");
    sm.commit();
    sm.advance_mutation(tx_id, MutationState::EXECUTED, "Candidate staged")
        .unwrap();

    // Degradation detected -> Roll back
    sm.advance_mutation(
        tx_id,
        MutationState::ROLLED_BACK,
        "Latency SLO violated in canary",
    )
    .unwrap();
    let rolled_back_snap = sm.rollback().expect("Must restore baseline snapshot");

    assert_eq!(rolled_back_snap.state_hash, snap.state_hash);
    assert_eq!(sm.get_key("version"), Some(&"v1.0.0".to_string()));
}

#[test]
fn test_state_machine_persistence_roundtrip() {
    let mut sm = StateMachine::new();
    let tx_id = sm.begin_mutation("test persistence");
    sm.advance_mutation(tx_id, MutationState::COMMITTED, "Done")
        .unwrap();

    let tmp_path = std::env::temp_dir().join(format!("ltn_test_tx_{}.json", Uuid::new_v4()));
    sm.save_to_file(&tmp_path).unwrap();

    let loaded_sm = StateMachine::load_from_file(&tmp_path);
    let _ = std::fs::remove_file(tmp_path);

    assert_eq!(loaded_sm.mutations.len(), 1);
    assert_eq!(
        loaded_sm.get_mutation(tx_id).unwrap().state,
        MutationState::COMMITTED
    );
}
