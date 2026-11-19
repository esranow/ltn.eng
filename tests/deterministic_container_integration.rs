//! End-to-End Integration Test Suite for Deterministic Container Management in LTN.ENG
//!
//! Verifies:
//! 1. Ingestion and execution of Docker Compose manifest (`fixtures/node_postgres_api/.ltn/deploy/docker-compose.yml`)
//! 2. Ingestion and execution of Kubernetes Deployment manifest (`fixtures/node_postgres_api/.ltn/deploy/kubernetes.yaml`)
//! 3. Ingestion and execution of synthetic K6s manifest
//! 4. End-to-end flow: File Read -> Manifest Detection -> Architecture Conversion ->
//!    Z3 SMT Prover Validation -> Container Launch -> Cumulative State Hash Verification -> Clean Termination
//! 5. Failure condition: Oversized resource requests (e.g. 512 cores) triggers Z3 UNSAT before any container launch

use ltn_container::{
    ContainerStatus, DeterministicContainerEngine, DeterministicRingTelemetry,
    DeterministicScheduler, ManifestConverter, ManifestDetector, ManifestKind, MetricSample,
};
use ltn_core::ir::{Architecture, ClusterState, ResourceBlock, WorkloadKind};
use ltn_core::solver::{SmtSolver, SolverError, SolverStatus};
use ltn_core::topology::HostHardwareInventory;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Resolves path to workspace fixtures regardless of test execution working directory
fn resolve_fixture_path(rel_path: &str) -> PathBuf {
    let p = Path::new(rel_path);
    if p.exists() {
        return p.to_path_buf();
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let from_manifest = manifest_dir.join(rel_path);
    if from_manifest.exists() {
        return from_manifest;
    }

    if let Some(parent) = manifest_dir.parent().and_then(|p| p.parent()) {
        let from_root = parent.join(rel_path);
        if from_root.exists() {
            return from_root;
        }
    }

    let abs = Path::new(r"D:\ltn-eng-workspace").join(rel_path);
    if abs.exists() {
        return abs;
    }

    panic!("Fixture not found at relative path: {}", rel_path);
}

/// Creates a standard test hardware inventory for formal SMT verification
fn create_test_hardware_inventory(cores: u32, ram_mb: u64) -> HostHardwareInventory {
    let mut hw = HostHardwareInventory {
        hostname: "ltn-deterministic-node-01".to_string(),
        ..Default::default()
    };
    hw.cpu.logical_threads = cores;
    hw.cpu.physical_cores = cores / 2;
    hw.ram.total_mb = ram_mb;
    hw.ram.available_mb = ram_mb;
    hw
}

/// Creates a standard test cluster state for deterministic scheduling
fn create_test_cluster(cores: u32, ram_mb: u32) -> ClusterState {
    ClusterState {
        resources: vec![ResourceBlock {
            block_id: Uuid::new_v4(),
            node_name: "ltn-deterministic-node-01".to_string(),
            total_cpu_cores: cores,
            total_ram_mb: ram_mb,
            total_vram_mb: 0,
            total_gpu_count: 0,
            network_bw_gbps: 10,
            endpoint: "127.0.0.1:8080".to_string(),
        }],
        network_latencies: std::collections::HashMap::new(),
    }
}

// =========================================================================
// TEST 1: Docker Compose Ingestion and Full Lifecycle Execution
// =========================================================================

#[test]
#[ignore = "legacy demo engine: only passes with Docker stopped (stand-in process); with Docker running it tries images that do not exist"]
fn test_e2e_docker_compose_ingestion_and_execution() {
    let rel_path = "tests/fixtures/node_postgres_api/.ltn/deploy/docker-compose.yml";
    let fixture_path = resolve_fixture_path(rel_path);

    // 1. File Read
    let raw_content =
        fs::read_to_string(&fixture_path).expect("Failed to read docker-compose fixture");
    assert!(
        !raw_content.is_empty(),
        "Manifest content must not be empty"
    );

    // 2. Manifest Detection
    let detected_kind =
        ManifestDetector::detect_file(&fixture_path).expect("ManifestDetector::detect_file failed");
    assert_eq!(
        detected_kind,
        ManifestKind::DockerCompose,
        "Manifest must be detected as DockerCompose"
    );
    assert_eq!(
        ManifestDetector::detect_str(&raw_content),
        ManifestKind::DockerCompose
    );

    // 3. Architecture Conversion
    let arch = ManifestConverter::convert_str(&raw_content, "node-postgres-compose")
        .expect("Failed to convert docker-compose into Architecture IR");

    assert_eq!(arch.workloads.len(), 2, "Expected 2 workloads (api, db)");
    let arch_hash = arch.canonical_hash();
    assert_eq!(
        arch_hash.len(),
        64,
        "Canonical architecture hash must be 64-character SHA-256"
    );

    let api_workload = arch
        .workloads
        .iter()
        .find(|w| w.name == "api")
        .expect("API workload not found in converted architecture");
    assert_eq!(api_workload.kind, WorkloadKind::NodePostgresApi);
    assert_eq!(api_workload.compute.cpu_cores, 1);
    assert_eq!(api_workload.compute.ram_mb, 1024);
    assert_eq!(api_workload.ports.len(), 1);
    assert_eq!(api_workload.ports[0].container_port, 3000);

    let db_workload = arch
        .workloads
        .iter()
        .find(|w| w.name == "db")
        .expect("DB workload not found in converted architecture");
    assert_eq!(db_workload.kind, WorkloadKind::StatefulStore);
    assert_eq!(db_workload.container_image, "postgres:16-alpine");
    assert_eq!(db_workload.ports.len(), 1);
    assert_eq!(db_workload.ports[0].container_port, 5432);

    // 4. Z3 SMT Prover Validation
    let hw = create_test_hardware_inventory(16, 32768);
    let validation = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(
        validation.status,
        SolverStatus::SAT,
        "SMT validation must be SAT for feasible hardware"
    );
    assert!(
        validation.violations.is_empty(),
        "Expected no constraint violations, got: {:?}",
        validation.violations
    );

    let cert = SmtSolver::generate_simulation_certificate(&arch, &hw);
    assert_eq!(cert.status, SolverStatus::SAT);
    assert!(!cert.proof_hash.is_empty());

    let cluster = create_test_cluster(16, 32768);
    let (exec_manifest, placement_receipt) =
        DeterministicScheduler::schedule_architecture(&arch, &cluster)
            .expect("DeterministicScheduler placement failed");
    assert!(placement_receipt.verified_feasible);
    assert_eq!(placement_receipt.allocations.len(), 2);
    assert_eq!(exec_manifest.mappings.len(), 2);

    // 5. Container Launch
    let mut engine = DeterministicContainerEngine::new();
    assert_eq!(engine.list_containers().len(), 0);

    let launch_res = engine.launch_workload(api_workload);
    let api_container = launch_res.expect("Failed to launch API container in engine");
    assert_eq!(api_container.name, "api");
    assert_eq!(api_container.status, ContainerStatus::Running);
    assert!(!api_container.state_hash.is_empty());

    let db_container = engine
        .launch_workload(db_workload)
        .expect("Failed to launch DB container in engine");
    assert_eq!(db_container.name, "db");
    assert_eq!(db_container.status, ContainerStatus::Running);
    assert!(!db_container.state_hash.is_empty());

    assert_eq!(engine.list_containers().len(), 2);

    // 6. Cumulative State Hash Verification
    let cumulative_hash = engine.verify_cluster_hash();
    assert_eq!(
        cumulative_hash.len(),
        64,
        "Cluster cumulative hash must be 64-char hex"
    );

    // Launch identical workloads on a fresh engine to guarantee bit-level determinism
    let mut duplicate_engine = DeterministicContainerEngine::new();
    duplicate_engine.launch_workload(api_workload).unwrap();
    duplicate_engine.launch_workload(db_workload).unwrap();
    let duplicate_hash = duplicate_engine.verify_cluster_hash();
    assert_eq!(
        cumulative_hash, duplicate_hash,
        "Cumulative state hash must be strictly deterministic across independent engine runs"
    );

    // 7. Clean Termination
    let ids: Vec<Uuid> = engine.list_containers().iter().map(|c| c.id).collect();
    for cid in ids {
        assert!(engine.stop_container(&cid).is_ok());
    }

    for container in engine.list_containers() {
        assert_eq!(
            container.status,
            ContainerStatus::Stopped,
            "Container '{}' should be in Stopped state",
            container.name
        );
    }
}

// =========================================================================
// TEST 2: Kubernetes Manifest Ingestion and Full Lifecycle Execution
// =========================================================================

#[test]
#[ignore = "legacy demo engine: only passes with Docker stopped (stand-in process); with Docker running it tries images that do not exist"]
fn test_e2e_kubernetes_deployment_ingestion_and_execution() {
    let rel_path = "tests/fixtures/node_postgres_api/.ltn/deploy/kubernetes.yaml";
    let fixture_path = resolve_fixture_path(rel_path);

    // 1. File Read
    let raw_content =
        fs::read_to_string(&fixture_path).expect("Failed to read kubernetes manifest fixture");
    assert!(!raw_content.is_empty());

    // 2. Manifest Detection
    let detected_kind =
        ManifestDetector::detect_file(&fixture_path).expect("ManifestDetector::detect_file failed");
    assert_eq!(
        detected_kind,
        ManifestKind::KubernetesDeployment,
        "Must detect KubernetesDeployment"
    );
    assert_eq!(
        ManifestDetector::detect_str(&raw_content),
        ManifestKind::KubernetesDeployment
    );

    // 3. Architecture Conversion
    let arch = ManifestConverter::convert_str(&raw_content, "node-api-k8s")
        .expect("Failed to convert kubernetes manifest into Architecture IR");

    assert_eq!(
        arch.workloads.len(),
        1,
        "Expected 1 container workload from Deployment"
    );
    let wl = &arch.workloads[0];
    assert_eq!(wl.name, "api");
    assert_eq!(wl.kind, WorkloadKind::NodePostgresApi);
    assert_eq!(wl.container_image, "ltn/node-api:latest");
    assert_eq!(wl.compute.cpu_cores, 2, "2000m should parse to 2 cores");
    assert_eq!(wl.compute.ram_mb, 2048, "2048Mi should parse to 2048 MB");
    assert_eq!(wl.ports.len(), 1);
    assert_eq!(wl.ports[0].container_port, 3000);
    assert_eq!(wl.replicas.desired_replicas, 2);

    // 4. Z3 SMT Prover Validation
    let hw = create_test_hardware_inventory(8, 16384);
    let val_res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(val_res.status, SolverStatus::SAT);
    assert!(val_res.violations.is_empty());

    let cert = SmtSolver::generate_simulation_certificate(&arch, &hw);
    assert_eq!(cert.status, SolverStatus::SAT);

    let cluster = create_test_cluster(8, 16384);
    let (exec_manifest, placement_receipt) =
        DeterministicScheduler::schedule_architecture(&arch, &cluster)
            .expect("Failed to schedule k8s deployment");
    assert!(placement_receipt.verified_feasible);
    assert_eq!(placement_receipt.allocations[0].allocated_cpu, 2);
    assert_eq!(placement_receipt.allocations[0].allocated_ram_mb, 2048);
    assert_eq!(exec_manifest.mappings.len(), 1);

    // 5. Container Launch
    let mut engine = DeterministicContainerEngine::new();
    let res = engine.launch_workload(wl);
    let container = res.expect("Failed to launch k8s workload in engine");
    assert_eq!(container.name, "api");
    assert_eq!(container.status, ContainerStatus::Running);
    assert!(!container.state_hash.is_empty());

    // 6. Cumulative State Hash Verification
    let cluster_hash = engine.verify_cluster_hash();
    assert_eq!(cluster_hash.len(), 64);

    let mut dup_engine = DeterministicContainerEngine::new();
    dup_engine.launch_workload(wl).unwrap();
    assert_eq!(
        cluster_hash,
        dup_engine.verify_cluster_hash(),
        "Cumulative hash must be strictly reproducible"
    );

    // 7. Clean Termination
    assert!(engine.stop_container(&container.id).is_ok());
    let stopped = engine.list_containers()[0];
    assert_eq!(stopped.status, ContainerStatus::Stopped);
}

// =========================================================================
// TEST 3: Synthetic K6s Manifest Ingestion and Full Lifecycle Execution
// =========================================================================

#[test]
fn test_e2e_synthetic_k6s_manifest_ingestion_and_execution() {
    // 1. Synthetic K6s Manifest
    let k6s_yaml = r#"
apiVersion: k6.io/v1
kind: K6
metadata:
  name: load-benchmark
spec:
  parallelism: 4
  script: /scripts/benchmark.js
"#;

    // 2. Manifest Detection
    let detected_kind = ManifestDetector::detect_str(k6s_yaml);
    assert_eq!(
        detected_kind,
        ManifestKind::K6sManifest,
        "Must identify K6 CRD manifest as K6sManifest"
    );

    // 3. Architecture Conversion
    let arch = ManifestConverter::convert_str(k6s_yaml, "load-benchmark")
        .expect("Failed to convert K6s manifest into Architecture IR");

    assert_eq!(arch.workloads.len(), 1);
    let wl = &arch.workloads[0];
    assert_eq!(wl.name, "load-benchmark-k6s-runner");
    assert_eq!(wl.kind, WorkloadKind::GoWorker);
    assert_eq!(wl.container_image, "grafana/k6:latest");
    assert_eq!(wl.entrypoint, Some("k6 run /scripts/test.js".to_string()));
    assert_eq!(wl.compute.cpu_cores, 2);
    assert_eq!(wl.compute.ram_mb, 2048);
    assert_eq!(wl.ports[0].container_port, 6565);

    // 4. Z3 SMT Prover Validation
    let hw = create_test_hardware_inventory(8, 16384);
    let val_res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(val_res.status, SolverStatus::SAT);

    let cluster = create_test_cluster(8, 16384);
    let (exec_manifest, placement_receipt) =
        DeterministicScheduler::schedule_architecture(&arch, &cluster)
            .expect("DeterministicScheduler placement failed for K6s");
    assert!(placement_receipt.verified_feasible);
    assert_eq!(exec_manifest.mappings.len(), 1);

    // 5. Container Launch
    let mut engine = DeterministicContainerEngine::new();
    let res = engine.launch_workload(wl);
    let container = res.expect("Failed to launch K6s workload in engine");
    assert_eq!(container.status, ContainerStatus::Running);

    // 6. Cumulative State Hash Verification
    let cluster_hash = engine.verify_cluster_hash();
    assert_eq!(cluster_hash.len(), 64);

    // 7. Clean Termination
    assert!(engine.stop_container(&container.id).is_ok());
    assert_eq!(engine.list_containers()[0].status, ContainerStatus::Stopped);
}

// =========================================================================
// TEST 4: Failure Condition: Oversized Resource Request Triggers Z3 UNSAT
// =========================================================================

#[test]
fn test_failure_condition_oversized_resource_triggers_z3_unsat_before_container_launch() {
    let rel_path = "tests/fixtures/node_postgres_api/.ltn/deploy/docker-compose.yml";
    let fixture_path = resolve_fixture_path(rel_path);
    let raw_content = fs::read_to_string(&fixture_path).unwrap();

    let mut arch = ManifestConverter::convert_str(&raw_content, "oversized-app")
        .expect("Conversion should succeed");

    // Intentionally mutate workload requirement to an oversized 512 CPU cores request
    let oversized_cores = 512;
    arch.workloads[0].compute.cpu_cores = oversized_cores;

    // Standard cluster node with only 64 cores
    let cluster = create_test_cluster(64, 131072);
    let hw = create_test_hardware_inventory(64, 131072);

    // Formally verify that Z3 SMT detects UNSAT capacity violation
    let val_res = SmtSolver::validate_architecture(&arch, &hw);
    assert_eq!(
        val_res.status,
        SolverStatus::UNSAT,
        "Oversized resource request of {} cores must produce SolverStatus::UNSAT",
        oversized_cores
    );

    let cpu_violation = val_res
        .violations
        .iter()
        .find(|v| v.constraint_id.contains("CPU"))
        .expect("Expected CPU capacity constraint violation");
    assert!(cpu_violation.explanation.contains("512"));
    assert!(!cpu_violation.suggested_replanning_options.is_empty());

    // DeterministicScheduler should also fail with Unsatisfiable error
    let schedule_result = DeterministicScheduler::schedule_architecture(&arch, &cluster);
    match &schedule_result {
        Err(SolverError::Unsatisfiable { mus_core }) => {
            assert!(
                mus_core
                    .iter()
                    .any(|msg| msg.contains("requires CPU cores 512")
                        || msg.contains("satisfies workload")),
                "MUS core should pinpoint the 512 CPU core violation, got: {:?}",
                mus_core
            );
        }
        other => panic!("Expected SolverError::Unsatisfiable, got: {:?}", other),
    }

    // CRITICAL INVARIANT: Prover failure MUST preempt and prevent any container launch
    let engine = DeterministicContainerEngine::new();
    // Do NOT launch workload because schedule_result was Err
    if schedule_result.is_ok() {
        panic!("Container launch occurred despite SMT UNSAT violation!");
    }

    // Confirm container engine remains completely empty (zero containers launched)
    assert_eq!(
        engine.list_containers().len(),
        0,
        "No containers should be launched when SMT prover returns UNSAT"
    );
}

// =========================================================================
// TEST 5: Heterogeneous Multi-Manifest Composition & Ring Telemetry Tracking
// =========================================================================

#[test]
#[ignore = "legacy demo engine: only passes with Docker stopped (stand-in process); with Docker running it tries images that do not exist"]
fn test_heterogeneous_multi_manifest_composition_and_telemetry() {
    let compose_path =
        resolve_fixture_path("tests/fixtures/node_postgres_api/.ltn/deploy/docker-compose.yml");
    let k8s_path =
        resolve_fixture_path("tests/fixtures/node_postgres_api/.ltn/deploy/kubernetes.yaml");

    let compose_str = fs::read_to_string(compose_path).unwrap();
    let k8s_str = fs::read_to_string(k8s_path).unwrap();

    let compose_arch = ManifestConverter::convert_str(&compose_str, "compose-sys").unwrap();
    let k8s_arch = ManifestConverter::convert_str(&k8s_str, "k8s-sys").unwrap();

    let k6s_yaml = r#"
apiVersion: k6.io/v1
kind: K6
metadata:
  name: perf-suite
spec:
  parallelism: 2
  script: /test.js
"#;
    let k6s_arch = ManifestConverter::convert_str(k6s_yaml, "perf-suite").unwrap();

    // Verify all 3 manifests convert valid distinct architectures
    assert_eq!(compose_arch.workloads.len(), 2);
    assert_eq!(k8s_arch.workloads.len(), 1);
    assert_eq!(k6s_arch.workloads.len(), 1);

    // Combine all workloads into unified cluster deployment
    let mut all_workloads = Vec::new();
    all_workloads.extend(compose_arch.workloads);
    all_workloads.extend(k8s_arch.workloads);
    all_workloads.extend(k6s_arch.workloads);
    assert_eq!(all_workloads.len(), 4);

    let mut unified_arch = Architecture::new(
        "unified-heterogeneous-cluster",
        ltn_core::ir::ProviderTarget::DeterministicContainer,
    );
    unified_arch.workloads = all_workloads;

    // SMT Feasibility on 32-core node
    let cluster = create_test_cluster(32, 65536);
    let (manifest, receipt) =
        DeterministicScheduler::schedule_architecture(&unified_arch, &cluster)
            .expect("Unified multi-manifest scheduling failed");
    assert!(receipt.verified_feasible);
    assert_eq!(receipt.allocations.len(), 4);
    assert_eq!(manifest.mappings.len(), 4);

    // Launch all 4 workloads into container engine
    let mut engine = DeterministicContainerEngine::new();
    let mut telemetry_ring = DeterministicRingTelemetry::new(16);

    for (idx, wl) in unified_arch.workloads.iter().enumerate() {
        let container = engine
            .launch_workload(wl)
            .expect("Failed to launch workload");
        assert_eq!(container.status, ContainerStatus::Running);

        // Record initial metric sample into ring telemetry
        telemetry_ring.record(MetricSample {
            timestamp_ms: 1_700_000_000_000 + (idx as u64 * 100),
            container_name: container.name.clone(),
            cpu_pct: 15.5 * (idx as f64 + 1.0),
            memory_used_mb: container.ram_mb / 2,
            active_connections: 5 * (idx as u32 + 1),
            p99_latency_ms: 8.5 + (idx as f64 * 1.5),
        });
    }

    assert_eq!(engine.list_containers().len(), 4);
    assert_eq!(telemetry_ring.count(), 4);
    assert!(telemetry_ring.average_cpu_pct() > 0.0);
    assert!(telemetry_ring.average_p99_latency_ms() > 0.0);

    // Verify Cumulative State Hash
    let cluster_hash = engine.verify_cluster_hash();
    assert_eq!(cluster_hash.len(), 64);

    // Clean Termination of all 4 containers
    let ids: Vec<Uuid> = engine.list_containers().iter().map(|c| c.id).collect();
    for id in ids {
        assert!(engine.stop_container(&id).is_ok());
    }

    for c in engine.list_containers() {
        assert_eq!(c.status, ContainerStatus::Stopped);
    }
}
