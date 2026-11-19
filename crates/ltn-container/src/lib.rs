pub mod manifest;
pub mod runtime;
pub mod supervisor;

pub use manifest::{
    ComposeParser, ComposeSpec, K8sDeployment, K8sParser, K8sPod, ManifestConverter,
    ManifestDetector, ManifestKind,
};
pub use runtime::{
    ContainerStatus, DeterministicContainer, DeterministicContainerEngine, ProcessSandbox,
    ProcessSandboxLimits,
};
pub use supervisor::{
    DeterministicRingTelemetry, DeterministicScheduler, MetricSample, ScheduledPlacementReceipt,
    WorkloadAllocation,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ltn_core::ir::{ClusterState, ResourceBlock, WorkloadKind};
    use uuid::Uuid;

    #[test]
    fn test_manifest_detection_docker_compose() {
        let yaml = r#"
version: '3.8'
services:
  web:
    image: nginx:alpine
    ports:
      - "80:80"
"#;
        assert_eq!(
            ManifestDetector::detect_str(yaml),
            ManifestKind::DockerCompose
        );
    }

    #[test]
    fn test_manifest_detection_kubernetes_deployment() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api-deploy
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: api
        image: node:20
"#;
        assert_eq!(
            ManifestDetector::detect_str(yaml),
            ManifestKind::KubernetesDeployment
        );
    }

    #[test]
    fn test_manifest_detection_kubernetes_pod() {
        let yaml = r#"
apiVersion: v1
kind: Pod
metadata:
  name: lone-pod
spec:
  containers:
  - name: worker
    image: alpine:latest
"#;
        assert_eq!(
            ManifestDetector::detect_str(yaml),
            ManifestKind::KubernetesPod
        );
    }

    #[test]
    fn test_manifest_detection_k6s() {
        let yaml = r#"
apiVersion: k6.io/v1
kind: K6
metadata:
  name: load-test
spec:
  parallelism: 4
  script: test.js
"#;
        assert_eq!(
            ManifestDetector::detect_str(yaml),
            ManifestKind::K6sManifest
        );
    }

    #[test]
    fn test_compose_parser_and_converter() {
        let yaml = r#"
version: '3.8'
services:
  api:
    image: my-node-api:latest
    ports:
      - "3000:3000"
    deploy:
      resources:
        limits:
          cpus: "2.0"
          memory: "1024M"
  db:
    image: postgres:16
    ports:
      - "5432:5432"
    deploy:
      resources:
        limits:
          cpus: "4.0"
          memory: "4096M"
"#;
        let arch = ManifestConverter::convert_str(yaml, "fullstack-app").unwrap();
        assert_eq!(arch.workloads.len(), 2);

        let api_wl = arch.workloads.iter().find(|w| w.name == "api").unwrap();
        assert_eq!(api_wl.compute.cpu_cores, 2);
        assert_eq!(api_wl.compute.ram_mb, 1024);
        assert_eq!(api_wl.ports[0].container_port, 3000);

        let db_wl = arch.workloads.iter().find(|w| w.name == "db").unwrap();
        assert_eq!(db_wl.compute.cpu_cores, 4);
        assert_eq!(db_wl.compute.ram_mb, 4096);
        assert_eq!(db_wl.kind, WorkloadKind::StatefulStore);
    }

    #[test]
    fn test_k8s_parser_and_converter() {
        let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: payments-service
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: payments-api
        image: payments:v1
        ports:
        - containerPort: 8080
        resources:
          limits:
            cpu: "2000m"
            memory: "2048Mi"
"#;
        let arch = ManifestConverter::convert_str(yaml, "payments-app").unwrap();
        assert_eq!(arch.workloads.len(), 1);

        let wl = &arch.workloads[0];
        assert_eq!(wl.name, "payments-api");
        assert_eq!(wl.compute.cpu_cores, 2);
        assert_eq!(wl.compute.ram_mb, 2048);
        assert_eq!(wl.ports[0].container_port, 8080);
        assert_eq!(wl.replicas.desired_replicas, 3);
    }

    #[test]
    fn test_deterministic_scheduler_success() {
        let cluster = ClusterState {
            resources: vec![ResourceBlock {
                block_id: Uuid::new_v4(),
                node_name: "prod-node-1".to_string(),
                total_cpu_cores: 16,
                total_ram_mb: 32768,
                total_vram_mb: 0,
                total_gpu_count: 0,
                network_bw_gbps: 10,
                endpoint: "127.0.0.1:8080".to_string(),
            }],
            network_latencies: std::collections::HashMap::new(),
        };

        let yaml = r#"
version: '3.8'
services:
  web:
    image: nginx:alpine
    ports:
      - "80:80"
    deploy:
      resources:
        limits:
          cpus: "2"
          memory: "1024M"
"#;
        let arch = ManifestConverter::convert_str(yaml, "web-app").unwrap();
        let (manifest, receipt) =
            DeterministicScheduler::schedule_architecture(&arch, &cluster).unwrap();

        assert_eq!(manifest.mappings.len(), 1);
        assert!(receipt.verified_feasible);
        assert_eq!(receipt.allocations.len(), 1);
        assert_eq!(receipt.allocations[0].allocated_cpu, 2);
        assert_eq!(receipt.allocations[0].allocated_ram_mb, 1024);
    }

    #[test]
    fn test_deterministic_container_engine_lifecycle() {
        let mut engine = DeterministicContainerEngine::new();
        assert_eq!(engine.list_containers().len(), 0);

        let yaml = r#"
version: '3.8'
services:
  worker:
    image: worker:latest
    ports:
      - "9000:9000"
    deploy:
      resources:
        limits:
          cpus: "1"
          memory: "512M"
"#;
        let arch = ManifestConverter::convert_str(yaml, "worker-job").unwrap();
        let res = engine.launch_workload(&arch.workloads[0]);
        assert!(res.is_ok(), "Engine should launch workload seamlessly (via Docker or native sandbox): {:?}", res.err());
        let container = res.unwrap();
        assert_eq!(container.name, "worker");
        assert_eq!(container.status, ContainerStatus::Running);
        assert!(!container.state_hash.is_empty());
        assert_eq!(engine.list_containers().len(), 1);

        let initial_hash = engine.verify_cluster_hash();
        assert_eq!(initial_hash.len(), 64);

        // Stop container
        assert!(engine.stop_container(&container.id).is_ok());
        let updated = engine.list_containers()[0];
        assert_eq!(updated.status, ContainerStatus::Stopped);
    }

    #[test]
    fn test_deterministic_ring_telemetry() {
        let mut ring = DeterministicRingTelemetry::new(4);
        assert_eq!(ring.count(), 0);

        for i in 1..=6 {
            ring.record(MetricSample {
                timestamp_ms: 1000 + i as u64,
                container_name: "test-container".to_string(),
                cpu_pct: (i * 10) as f64,
                memory_used_mb: 256,
                active_connections: i as u32,
                p99_latency_ms: 10.0 + (i as f64 * 2.0),
            });
        }

        // Circular capacity bounded at 4
        assert_eq!(ring.count(), 4);
        assert!(ring.average_cpu_pct() > 0.0);
        assert!(ring.average_p99_latency_ms() > 0.0);
    }
}
