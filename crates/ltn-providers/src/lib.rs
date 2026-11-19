pub mod actuators;
pub mod auth;

pub use actuators::DeterministicContainerActuator;

#[cfg(test)]
mod tests {
    use super::*;
    use actuators::{
        AwsActuator, AzureActuator, BareMetalActuator, DeterministicContainerActuator,
        DockerActuator, DockerNetworkDriver, GcpActuator, LocalOsActuator,
    };
    use auth::{AuthChecker, AuthStatus, ProviderAuthError};
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    struct EnvRestorer {
        vars: Vec<(&'static str, Option<String>)>,
    }

    impl EnvRestorer {
        fn new(keys: &[&'static str]) -> Self {
            let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
            Self { vars }
        }
    }

    impl Drop for EnvRestorer {
        fn drop(&mut self) {
            for (key, val) in &self.vars {
                if let Some(v) = val {
                    std::env::set_var(key, v);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    // ==========================================
    // 1. AuthChecker Tests for all 6 Providers
    // ==========================================

    #[test]
    fn test_auth_detection_local_docker() {
        assert_eq!(
            AuthChecker::check_provider_auth("LOCAL_DOCKER"),
            AuthStatus::Authenticated
        );
        assert_eq!(
            AuthChecker::check_provider_auth("local_docker"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_local_os() {
        assert_eq!(
            AuthChecker::check_provider_auth("LOCAL_OS"),
            AuthStatus::Authenticated
        );
        assert_eq!(
            AuthChecker::check_provider_auth("local_os"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_bare_metal() {
        assert_eq!(
            AuthChecker::check_provider_auth("BARE_METAL"),
            AuthStatus::Authenticated
        );
        assert_eq!(
            AuthChecker::check_provider_auth("bare_metal"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_aws_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_PROFILE", "AWS_ENDPOINT_URL"]);

        // Via AWS_ACCESS_KEY_ID
        std::env::remove_var("AWS_PROFILE");
        std::env::remove_var("AWS_ENDPOINT_URL");
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(
            AuthChecker::check_provider_auth("AWS"),
            AuthStatus::Authenticated
        );

        // Via AWS_PROFILE
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::set_var("AWS_PROFILE", "default");
        assert_eq!(
            AuthChecker::check_provider_auth("AWS"),
            AuthStatus::Authenticated
        );

        // Via AWS_ENDPOINT_URL
        std::env::remove_var("AWS_PROFILE");
        std::env::set_var("AWS_ENDPOINT_URL", "http://localhost:4566");
        assert_eq!(
            AuthChecker::check_provider_auth("AWS"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_aws_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_PROFILE", "AWS_ENDPOINT_URL"]);

        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_PROFILE");
        std::env::remove_var("AWS_ENDPOINT_URL");

        match AuthChecker::check_provider_auth("AWS") {
            AuthStatus::BlockedUnauthenticated(reason) => {
                assert!(reason.contains("AWS credentials missing"));
            }
            _ => panic!("Expected AWS to be BlockedUnauthenticated when env vars are absent"),
        }
    }

    #[test]
    fn test_auth_detection_gcp_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&[
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
        ]);

        // Via GOOGLE_APPLICATION_CREDENTIALS
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", "/path/to/creds.json");
        assert_eq!(
            AuthChecker::check_provider_auth("GCP"),
            AuthStatus::Authenticated
        );

        // Via GEMINI_API_KEY
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        std::env::set_var("GEMINI_API_KEY", "test-gemini-key");
        assert_eq!(
            AuthChecker::check_provider_auth("GCP"),
            AuthStatus::Authenticated
        );

        // Via GOOGLE_API_KEY
        std::env::remove_var("GEMINI_API_KEY");
        std::env::set_var("GOOGLE_API_KEY", "test-google-key");
        assert_eq!(
            AuthChecker::check_provider_auth("GCP"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_gcp_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&[
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GCP_ENDPOINT_URL",
        ]);

        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("GCP_ENDPOINT_URL");

        match AuthChecker::check_provider_auth("GCP") {
            AuthStatus::BlockedUnauthenticated(reason) => {
                assert!(reason.contains("GCP credentials missing"));
            }
            _ => panic!("Expected GCP to be BlockedUnauthenticated when env vars are absent"),
        }
    }

    #[test]
    fn test_auth_detection_azure_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AZURE_SUBSCRIPTION_ID", "AZURE_CLIENT_ID"]);

        // Via AZURE_SUBSCRIPTION_ID
        std::env::remove_var("AZURE_CLIENT_ID");
        std::env::set_var("AZURE_SUBSCRIPTION_ID", "sub-12345");
        assert_eq!(
            AuthChecker::check_provider_auth("AZURE"),
            AuthStatus::Authenticated
        );

        // Via AZURE_CLIENT_ID
        std::env::remove_var("AZURE_SUBSCRIPTION_ID");
        std::env::set_var("AZURE_CLIENT_ID", "client-12345");
        assert_eq!(
            AuthChecker::check_provider_auth("AZURE"),
            AuthStatus::Authenticated
        );
    }

    #[test]
    fn test_auth_detection_azure_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&[
            "AZURE_SUBSCRIPTION_ID",
            "AZURE_CLIENT_ID",
            "AZURE_ENDPOINT_URL",
        ]);

        std::env::remove_var("AZURE_SUBSCRIPTION_ID");
        std::env::remove_var("AZURE_CLIENT_ID");
        std::env::remove_var("AZURE_ENDPOINT_URL");

        match AuthChecker::check_provider_auth("AZURE") {
            AuthStatus::BlockedUnauthenticated(reason) => {
                assert!(reason.contains("Azure credentials missing"));
            }
            _ => panic!("Expected Azure to be BlockedUnauthenticated when env vars are absent"),
        }
    }

    #[test]
    fn test_auth_unknown_provider() {
        let status = AuthChecker::check_provider_auth("DIGITAL_OCEAN");
        match status {
            AuthStatus::BlockedUnauthenticated(reason) => {
                assert!(reason.contains("Unknown provider 'DIGITAL_OCEAN'"));
            }
            _ => panic!("Expected unknown provider to be blocked"),
        }
    }

    #[test]
    fn test_provider_auth_error_type() {
        let err = ProviderAuthError::BlockedUnauthenticated("test reason".to_string());
        assert_eq!(format!("{}", err), "BLOCKED_UNAUTHENTICATED: test reason");
    }

    // ==========================================
    // 2. Docker Actuator Tests
    // ==========================================

    #[test]
    fn test_docker_actuator_bridge_network() {
        let net =
            DockerActuator::create_standard_network("ltn-bridge-net", DockerNetworkDriver::Bridge);
        assert_eq!(net.network_name, "ltn-bridge-net");
        assert_eq!(net.driver, DockerNetworkDriver::Bridge);
        assert_eq!(net.subnet, "172.28.0.0/16");
        assert!(net.is_isolated);
    }

    #[test]
    fn test_docker_actuator_overlay_network() {
        let net = DockerActuator::create_standard_network(
            "ltn-overlay-net",
            DockerNetworkDriver::Overlay,
        );
        assert_eq!(net.network_name, "ltn-overlay-net");
        assert_eq!(net.driver, DockerNetworkDriver::Overlay);
        assert_eq!(net.subnet, "172.28.0.0/16");
        assert!(net.is_isolated);
    }

    #[test]
    fn test_docker_actuator_custom_network() {
        let net = DockerActuator::create_standard_network(
            "ltn-custom-net",
            DockerNetworkDriver::Custom("macvlan".to_string()),
        );
        assert_eq!(net.network_name, "ltn-custom-net");
        assert_eq!(
            net.driver,
            DockerNetworkDriver::Custom("macvlan".to_string())
        );
    }

    #[test]
    fn test_docker_container_deployment_default_network() {
        let res = DockerActuator::deploy_container("redis:alpine", "cache-node", "6379:6379");
        if DockerActuator::is_docker_available() {
            let res = res.expect("Docker deployment should succeed when daemon is online");
            assert!(res.container_id.starts_with("doc-") || !res.container_id.is_empty());
            assert_eq!(res.network_attached, "ltn-bridge");
            assert!(res.status.contains("cache-node"));
            assert!(res.status.contains("redis:alpine"));
            assert!(res.status.contains("6379:6379"));
        } else {
            assert!(res.is_err(), "Must return Err when Docker daemon is offline");
            let err = res.unwrap_err();
            assert!(
                err.contains("Docker daemon is offline"),
                "Error should indicate Docker daemon is offline, got: {}",
                err
            );
        }
    }

    #[test]
    fn test_docker_container_deployment_custom_network() {
        let net =
            DockerActuator::create_standard_network("ltn-custom-net", DockerNetworkDriver::Bridge);
        let res = DockerActuator::deploy_container_on_network(
            "nginx:alpine",
            "web-gateway",
            "8080:80",
            &net.network_name,
        );
        if DockerActuator::is_docker_available() {
            let res = res.expect("Docker deployment should succeed when daemon is online");
            assert!(res.container_id.starts_with("doc-") || !res.container_id.is_empty());
            assert_eq!(res.network_attached, "ltn-custom-net");
            assert!(res.status.contains("web-gateway"));
            assert!(res.status.contains("nginx:alpine"));
            assert!(res.status.contains("8080:80"));
        } else {
            assert!(res.is_err(), "Must return Err when Docker daemon is offline");
            let err = res.unwrap_err();
            assert!(
                err.contains("Docker daemon is offline"),
                "Error should indicate Docker daemon is offline, got: {}",
                err
            );
        }
    }

    #[test]
    fn test_docker_container_id_uniqueness() {
        let c1 = DockerActuator::deploy_container("alpine:latest", "c1", "8001:80");
        let c2 = DockerActuator::deploy_container("alpine:latest", "c2", "8002:80");
        if DockerActuator::is_docker_available() {
            let c1 = c1.unwrap();
            let c2 = c2.unwrap();
            assert_ne!(c1.container_id, c2.container_id);
        } else {
            assert!(c1.is_err());
            assert!(c2.is_err());
            assert!(c1.unwrap_err().contains("Docker daemon is offline"));
            assert!(c2.unwrap_err().contains("Docker daemon is offline"));
        }
    }

    // ==========================================
    // 3. GCP and Azure Actuator Contracts
    // ==========================================

    #[test]
    fn test_gcp_actuator_contract_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _restorer = EnvRestorer::new(&["GEMINI_API_KEY", "GCP_ENDPOINT_URL"]);
        std::env::set_var("GEMINI_API_KEY", "test-gcp-key");
        std::env::set_var("GCP_ENDPOINT_URL", "http://127.0.0.1:4566");

        let res = GcpActuator::deploy_gke_workload("payment-service");
        assert!(res.is_ok());
        let result = res.unwrap();
        assert!(result.cluster_id.starts_with("gke-"));
        assert_eq!(result.region, "us-central1");
        assert!(result.status.contains("payment-service"));
    }

    #[test]
    fn test_gcp_actuator_contract_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _restorer = EnvRestorer::new(&[
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GCP_ENDPOINT_URL",
        ]);
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("GCP_ENDPOINT_URL");

        let res = GcpActuator::deploy_gke_workload("payment-service");
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.contains("GCP credentials missing in environment"));
    }

    #[test]
    fn test_azure_actuator_contract_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AZURE_SUBSCRIPTION_ID", "AZURE_ENDPOINT_URL"]);
        std::env::set_var("AZURE_SUBSCRIPTION_ID", "sub-azure-test");
        std::env::set_var("AZURE_ENDPOINT_URL", "http://127.0.0.1:4566");

        let res = AzureActuator::deploy_aks_cluster("ltn-k8s-cluster");
        assert!(res.is_ok());
        let result = res.unwrap();
        assert_eq!(result.resource_group, "rg-ltn-prod");
        assert!(result.status.contains("ltn-k8s-cluster"));
    }

    #[test]
    fn test_azure_actuator_contract_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _restorer = EnvRestorer::new(&[
            "AZURE_SUBSCRIPTION_ID",
            "AZURE_CLIENT_ID",
            "AZURE_ENDPOINT_URL",
        ]);
        std::env::remove_var("AZURE_SUBSCRIPTION_ID");
        std::env::remove_var("AZURE_CLIENT_ID");
        std::env::remove_var("AZURE_ENDPOINT_URL");

        let res = AzureActuator::deploy_aks_cluster("ltn-k8s-cluster");
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.contains("Azure credentials missing in environment"));
    }

    #[test]
    fn test_aws_actuator_contract_authenticated() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL"]);
        std::env::set_var("AWS_ACCESS_KEY_ID", "dummy-key");
        std::env::remove_var("AWS_ENDPOINT_URL");

        let ecs = AwsActuator::deploy_ecs_task("auth-service");
        assert!(ecs.is_ok());
        let ecs_res = ecs.unwrap();
        assert!(ecs_res.instance_id.starts_with("i-"));
        assert!(ecs_res.status.contains("auth-service"));

        let s3 = AwsActuator::create_s3_bucket("ltn-assets");
        assert!(s3.is_ok());
        let s3_res = s3.unwrap();
        assert_eq!(s3_res.instance_id, "arn:aws:s3:::ltn-assets");

        let rds = AwsActuator::deploy_rds_instance("ltn-db");
        assert!(rds.is_ok());
        let rds_res = rds.unwrap();
        assert!(rds_res.instance_id.starts_with("rds-"));
    }

    #[test]
    fn test_aws_actuator_ministack_detection() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL"]);
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");

        let ecs = AwsActuator::deploy_ecs_task("local-task").unwrap();
        assert!(ecs.status.contains("HTTP 200 OK"));
        assert!(ecs.status.contains("local-task"));
    }

    #[test]
    fn test_aws_ministack_ecs_web_service() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL"]);
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");

        let res = AwsActuator::deploy_ecs_task("web-service");
        assert!(res.is_ok());
        let val = res.unwrap();
        assert!(val.instance_id.starts_with("i-"));
        assert!(val.status.contains("HTTP 200 OK"));
        assert!(val.status.contains("web-service"));
        assert_eq!(val.endpoint, "http://127.0.0.1:4566");
    }

    #[test]
    fn test_aws_ministack_rds_pg_prod() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL"]);
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");

        let res = AwsActuator::deploy_rds_instance("pg-prod");
        assert!(res.is_ok());
        let val = res.unwrap();
        assert!(val.instance_id.starts_with("rds-"));
        assert!(val.status.contains("HTTP 200 OK"));
        assert!(val.status.contains("pg-prod"));
        assert_eq!(val.endpoint, "http://127.0.0.1:4566");
    }

    #[test]
    fn test_aws_ministack_s3_bucket() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_ENDPOINT_URL"]);
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
        std::env::set_var("AWS_ENDPOINT_URL", "http://127.0.0.1:4566");

        let res = AwsActuator::create_s3_bucket("ltn-test-workload");
        assert!(res.is_ok());
        let val = res.unwrap();
        assert_eq!(val.instance_id, "arn:aws:s3:::ltn-test-workload");
        assert!(val.status.contains("HTTP 200 OK"));
        assert!(val.status.contains("ltn-test-workload"));
        assert_eq!(val.endpoint, "http://127.0.0.1:4566");
    }

    #[test]
    fn test_aws_actuator_blocked() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["AWS_ACCESS_KEY_ID", "AWS_PROFILE", "AWS_ENDPOINT_URL"]);
        std::env::remove_var("AWS_ACCESS_KEY_ID");
        std::env::remove_var("AWS_PROFILE");
        std::env::remove_var("AWS_ENDPOINT_URL");

        let res = AwsActuator::deploy_ecs_task("my-service");
        assert!(res.is_err());
        assert!(AwsActuator::create_s3_bucket("my-bucket").is_err());
        assert!(AwsActuator::deploy_rds_instance("my-db").is_err());
    }

    // ==========================================
    // 4. Local OS & Bare Metal Tests
    // ==========================================

    #[test]
    fn test_local_os_actuator_process_spawning() {
        let res = LocalOsActuator::spawn_process("echo hello");
        assert_eq!(res.pid, 12345);
        assert!(res.status.contains("echo hello"));
        assert!(res.status.contains("started on local OS"));

        let res2 = LocalOsActuator::spawn_process("systemctl restart nginx");
        assert!(res2.pid > 0);
        assert!(res2.status.contains("systemctl restart nginx"));
    }

    #[test]
    fn test_bare_metal_actuator_provisioning() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_SSH_USER", "LTN_SSH_KEY"]);
        std::env::remove_var("LTN_SSH_USER");
        std::env::remove_var("LTN_SSH_KEY");

        let res = BareMetalActuator::provision_node("192.168.1.50");
        assert_eq!(res.node_ip, "192.168.1.50");
        assert_eq!(res.status, "BLOCKED_SSH_UNAUTHENTICATED");
        assert_eq!(res.reachability, "UNREACHABLE_NO_CREDENTIALS");
        assert!(res.systemd_unit_staged.is_some());

        let local_res = BareMetalActuator::provision_node("127.0.0.1");
        assert_eq!(local_res.node_ip, "127.0.0.1");
        assert_eq!(local_res.reachability, "ONLINE_REACHABLE");
        assert!(local_res.status.contains("127.0.0.1"));
        assert!(local_res.status.contains("provisioned via systemd"));
    }

    #[test]
    fn test_bare_metal_actuator_discovery() {
        std::env::set_var(
            "LTN_BARE_METAL_TOPOLOGY",
            "D:/.ltn-graph/non_existent_fallback_sysinfo.json",
        );
        let report = BareMetalActuator::discover_nodes();
        assert_eq!(report.status, "DISCOVERY_COMPLETE");
        assert_eq!(report.total_nodes, report.discovered_nodes.len());
        assert!(!report.discovered_nodes.is_empty());

        let node0 = &report.discovered_nodes[0];
        assert_ne!(
            node0.node_id, "bm-node-01",
            "Must not return fake static bm-node-01"
        );
        assert!(node0.cpu_cores > 0, "Discovered CPU cores must be > 0");
        assert!(node0.memory_mb > 0, "Discovered RAM must be > 0");
        assert_eq!(node0.status, "ONLINE");
    }

    #[test]
    fn test_bare_metal_actuator_topology_file_multi_node() {
        if std::path::Path::new("D:/.ltn-graph/nodes.json").exists() {
            std::env::set_var("LTN_BARE_METAL_TOPOLOGY", "D:/.ltn-graph/nodes.json");
            let report = BareMetalActuator::discover_nodes();
            assert_eq!(report.total_nodes, 2);
            assert_eq!(report.discovered_nodes[0].node_id, "ceph-mon-01");
            assert_eq!(report.discovered_nodes[1].node_id, "ceph-osd-pool");
        }
    }

    #[test]
    fn test_bare_metal_actuator_discover_single_node() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let _restorer = EnvRestorer::new(&["LTN_SSH_USER", "LTN_SSH_KEY"]);
        std::env::remove_var("LTN_SSH_USER");
        std::env::remove_var("LTN_SSH_KEY");

        // Local node probe
        let node = BareMetalActuator::discover_node("127.0.0.1");
        assert!(node.node_id.starts_with("bm-"));
        assert_eq!(node.ip, "127.0.0.1");
        assert!(node.cpu_cores > 0);
        assert!(node.memory_mb > 0);
        assert_eq!(node.status, "HEALTHY");

        // Remote unauthenticated node probe
        let dead_node = BareMetalActuator::discover_node("192.0.2.1");
        assert_eq!(dead_node.status, "BLOCKED_SSH_UNAUTHENTICATED");
        assert_eq!(dead_node.cpu_cores, 0);
        assert_eq!(dead_node.memory_mb, 0);
    }

    // ==========================================================
    // DeterministicContainerActuator Integration & Unit Tests
    // ==========================================================

    #[test]
    fn test_deterministic_container_actuator_lifecycle() {
        use ltn_container::runtime::ContainerStatus;
        use ltn_core::ir::{
            ComputeRequirements, NetworkRequirements, ReplicaPolicy, StorageRequirements,
            WorkloadDefinition, WorkloadKind,
        };

        let mut actuator = DeterministicContainerActuator::new();
        assert_eq!(actuator.list_containers().len(), 0);

        let workload = WorkloadDefinition {
            id: uuid::Uuid::new_v4(),
            name: "deterministic-payment-svc".to_string(),
            kind: WorkloadKind::GenericContainer,
            container_image: "payment:v2".to_string(),
            entrypoint: None,
            ports: vec![],
            secrets: vec![],
            compute: ComputeRequirements {
                cpu_cores: 4,
                ram_mb: 4096,
                gpu_count: 0,
                vram_mb: 0,
                gpu_model_preference: None,
            },
            storage: StorageRequirements::default(),
            network: NetworkRequirements::default(),
            replicas: ReplicaPolicy::default(),
            placement: Default::default(),
        };

        // Launch workload
        let launch_res = actuator.launch_workload(&workload);
        let container = launch_res.expect("Launch must succeed via Docker or native sandbox");
        assert_eq!(container.name, "deterministic-payment-svc");
        assert_eq!(container.cpu_cores, 4);
        assert_eq!(container.ram_mb, 4096);
        assert_eq!(container.status, ContainerStatus::Running);
        assert!(!container.state_hash.is_empty());

        // Verify container state
        let status = actuator
            .verify_container_state(&container.id)
            .expect("Verification must succeed");
        assert_eq!(status, ContainerStatus::Running);
        assert!(actuator.is_running(&container.id));

        // Verify workload state hash match
        assert!(actuator.verify_workload_state(&workload).unwrap());

        // Verify cluster hash non-empty
        let cluster_hash = actuator.verify_cluster_hash();
        assert_eq!(cluster_hash.len(), 64);

        // Stop container
        actuator
            .stop_container(&container.id)
            .expect("Stop must succeed");
        let stopped_status = actuator
            .verify_container_state(&container.id)
            .expect("Status after stop");
        assert_eq!(stopped_status, ContainerStatus::Stopped);
        assert!(actuator.is_stopped(&container.id));
    }

    #[test]
    fn test_deterministic_container_actuator_deploy_result_and_state() {
        use ltn_container::runtime::ContainerStatus;
        use ltn_core::ir::{
            ComputeRequirements, NetworkRequirements, ReplicaPolicy, StorageRequirements,
            WorkloadDefinition, WorkloadKind,
        };

        let mut actuator = DeterministicContainerActuator::new();
        let workload = WorkloadDefinition {
            id: uuid::Uuid::new_v4(),
            name: "event-indexer".to_string(),
            kind: WorkloadKind::GoWorker,
            container_image: "indexer:latest".to_string(),
            entrypoint: None,
            ports: vec![],
            secrets: vec![],
            compute: ComputeRequirements {
                cpu_cores: 2,
                ram_mb: 1024,
                gpu_count: 0,
                vram_mb: 0,
                gpu_model_preference: None,
            },
            storage: StorageRequirements::default(),
            network: NetworkRequirements::default(),
            replicas: ReplicaPolicy::default(),
            placement: Default::default(),
        };

        let res = actuator.deploy_workload(&workload).expect("Deploy must succeed");
        assert_eq!(res.container_id, workload.id);
        assert_eq!(res.name, "event-indexer");
        assert_eq!(res.cpu_cores, 2);
        assert_eq!(res.ram_mb, 1024);
        assert_eq!(res.status, ContainerStatus::Running);
        assert!(res.verified);

        let verification = actuator
            .verify_container_state_detailed(&workload.id)
            .unwrap();
        assert!(verification.is_valid);
        assert_eq!(verification.status, ContainerStatus::Running);
    }
}
