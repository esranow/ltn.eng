pub mod architect;
pub mod artifacts;
pub mod classifier;
pub mod parsers;
pub mod planner;


#[cfg(test)]
mod tests {
    use super::*;
    use artifacts::MissingArtifactGenerator;
    use classifier::WorkloadClassifier;
    use ltn_core::ir::WorkloadKind;
    use parsers::ManifestParser;
    use planner::{ArchitecturePlanner, PlanTier};
    use std::path::Path;

    fn fixtures_base_dir() -> std::path::PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            manifest_dir.join("../../tests/fixtures"),
            manifest_dir.join("../tests/fixtures"),
            std::path::PathBuf::from("tests/fixtures"),
            std::path::PathBuf::from("D:/ltn-eng-workspace/tests/fixtures"),
        ];
        for candidate in &candidates {
            if candidate.exists() && candidate.is_dir() {
                return candidate.clone();
            }
        }
        panic!("tests/fixtures directory not found among candidates: {:?}", candidates);
    }

    struct TempTestDir {
        path: std::path::PathBuf,
    }

    impl TempTestDir {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ltn-test-{}-{}", prefix, uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_manifest_parser() {
        let current_dir = Path::new(".");
        let discovery = ManifestParser::discover_in_dir(current_dir);
        assert!(!discovery.detected_files.is_empty());
    }

    #[test]
    fn test_manifest_node_package_json() {
        let fixtures = fixtures_base_dir();

        // 1. node_postgres_api fixture
        let node_dir = fixtures.join("node_postgres_api");
        let discovery = ManifestParser::discover_in_dir(&node_dir);
        assert!(discovery.has_package_json, "node_postgres_api should have package.json");
        assert!(discovery.detected_files.contains(&"package.json".to_string()));
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_pyproject_toml);
        assert!(!discovery.has_docker_compose());

        let parsed = ManifestParser::parse_package_json(&node_dir).expect("should parse valid package.json");
        assert_eq!(parsed["name"], "node-postgres-api");
        assert_eq!(parsed["dependencies"]["express"], "^4.18.2");

        // 2. react_vite_static fixture
        let react_dir = fixtures.join("react_vite_static");
        let discovery_react = ManifestParser::discover_in_dir(&react_dir);
        assert!(discovery_react.has_package_json, "react_vite_static should have package.json");
        assert!(discovery_react.detected_files.contains(&"package.json".to_string()));

        let parsed_react = ManifestParser::parse_package_json(&react_dir).expect("should parse valid package.json");
        assert_eq!(parsed_react["name"], "react-vite-static");
        assert_eq!(parsed_react["dependencies"]["react"], "^18.2.0");
    }

    #[test]
    fn test_manifest_rust_cargo_toml() {
        let fixtures = fixtures_base_dir();
        let rust_dir = fixtures.join("rust_wasm");
        let discovery = ManifestParser::discover_in_dir(&rust_dir);

        assert!(discovery.has_cargo_toml, "rust_wasm should have Cargo.toml");
        assert!(discovery.detected_files.contains(&"Cargo.toml".to_string()));
        assert!(!discovery.has_package_json);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_pyproject_toml);
        assert!(!discovery.has_docker_compose());
    }

    #[test]
    fn test_manifest_python_pyproject_toml() {
        let fixtures = fixtures_base_dir();
        let py_dir = fixtures.join("python_vllm_gpu");
        let discovery = ManifestParser::discover_in_dir(&py_dir);

        assert!(discovery.has_pyproject_toml, "python_vllm_gpu should have pyproject.toml");
        assert!(discovery.detected_files.contains(&"pyproject.toml".to_string()) || discovery.detected_files.contains(&"requirements.txt".to_string()));
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_package_json);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_docker_compose());
    }

    #[test]
    fn test_manifest_go_mod() {
        let fixtures = fixtures_base_dir();
        let go_dir = fixtures.join("go_worker");
        let discovery = ManifestParser::discover_in_dir(&go_dir);

        assert!(discovery.has_go_mod, "go_worker should have go.mod");
        assert!(discovery.detected_files.contains(&"go.mod".to_string()));
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_package_json);
        assert!(!discovery.has_pyproject_toml);
        assert!(!discovery.has_docker_compose());
    }

    #[test]
    fn test_manifest_docker_compose() {
        let fixtures = fixtures_base_dir();
        let store_dir = fixtures.join("stateful_store");
        let discovery = ManifestParser::discover_in_dir(&store_dir);

        assert!(discovery.has_docker_compose(), "stateful_store should have docker-compose.yml");
        assert!(discovery.detected_files.contains(&"docker-compose.yml".to_string()));
        assert!(!discovery.has_package_json);
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_pyproject_toml);
    }

    #[test]
    fn test_manifest_failure_empty_directory() {
        let temp_dir = TempTestDir::new("empty");
        let discovery = ManifestParser::discover_in_dir(&temp_dir.path);

        assert!(!discovery.has_package_json);
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_pyproject_toml);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_pom_xml);
        assert!(!discovery.has_dockerfile);
        assert!(!discovery.has_docker_compose());
        assert!(discovery.detected_files.is_empty());
        assert!(discovery.is_empty());
    }

    #[test]
    fn test_manifest_failure_non_existent_directory() {
        let nonexistent = std::env::temp_dir().join(format!("ltn-nonexistent-{}", uuid::Uuid::new_v4()));
        assert!(!nonexistent.exists());

        let discovery = ManifestParser::discover_in_dir(&nonexistent);
        assert!(!discovery.has_package_json);
        assert!(!discovery.has_cargo_toml);
        assert!(!discovery.has_pyproject_toml);
        assert!(!discovery.has_go_mod);
        assert!(!discovery.has_pom_xml);
        assert!(!discovery.has_dockerfile);
        assert!(!discovery.has_docker_compose());
        assert!(discovery.detected_files.is_empty());
        assert!(discovery.is_empty());

        let parse_result = ManifestParser::parse_package_json(&nonexistent);
        assert!(parse_result.is_err());
    }

    #[test]
    fn test_manifest_failure_corrupt_json() {
        let temp_dir = TempTestDir::new("corrupt");
        let bad_json_path = temp_dir.path.join("package.json");
        std::fs::write(&bad_json_path, "{ \"name\": \"corrupt\", dependencies: { unclosed_bracket ").expect("write bad json");

        let discovery = ManifestParser::discover_in_dir(&temp_dir.path);
        // Corrupted JSON should NOT register as a valid package.json manifest
        assert!(!discovery.has_package_json, "Corrupt package.json must not be accepted as valid");
        assert!(!discovery.detected_files.contains(&"package.json".to_string()));
        assert!(discovery.detected_files.is_empty());

        // parse_package_json should explicitly return Err
        let parse_result = ManifestParser::parse_package_json(&temp_dir.path);
        assert!(parse_result.is_err(), "parse_package_json should fail on corrupt JSON");
        let err_msg = parse_result.unwrap_err();
        assert!(err_msg.contains("Corrupt JSON"), "Error should indicate corruption: {}", err_msg);
    }

    #[test]
    fn test_manifest_multi_manifest_coexistence() {
        let temp_dir = TempTestDir::new("multi-manifest");
        std::fs::write(temp_dir.path.join("package.json"), r#"{"name": "full-stack", "version": "1.0.0"}"#).unwrap();
        std::fs::write(temp_dir.path.join("Cargo.toml"), "[package]\nname = \"core\"\nversion = \"0.1.0\"").unwrap();
        std::fs::write(temp_dir.path.join("Dockerfile"), "FROM alpine:3.19\n").unwrap();
        std::fs::write(temp_dir.path.join("docker-compose.yml"), "version: '3'\nservices:\n  app:\n    build: .\n").unwrap();

        let discovery = ManifestParser::discover_in_dir(&temp_dir.path);
        assert!(discovery.has_package_json);
        assert!(discovery.has_cargo_toml);
        assert!(discovery.has_dockerfile);
        assert!(discovery.has_docker_compose());
        assert_eq!(discovery.detected_files.len(), 4);
    }

    #[test]
    fn test_manifest_all_fixtures_matrix() {
        let fixtures = fixtures_base_dir();
        let matrix = [
            ("go_worker", "go.mod", true, false, false, false, false),
            ("node_postgres_api", "package.json", false, true, false, false, false),
            ("python_vllm_gpu", "pyproject.toml", false, false, true, false, false),
            ("react_vite_static", "package.json", false, true, false, false, false),
            ("rust_wasm", "Cargo.toml", false, false, false, true, false),
            ("stateful_store", "docker-compose.yml", false, false, false, false, true),
        ];

        for (fixture_name, expected_file, exp_go, exp_node, exp_py, exp_rust, exp_compose) in matrix {
            let fixture_path = fixtures.join(fixture_name);
            assert!(fixture_path.exists(), "Fixture path {:?} must exist", fixture_path);

            let discovery = ManifestParser::discover_in_dir(&fixture_path);
            assert!(
                discovery.detected_files.contains(&expected_file.to_string()),
                "Fixture {} should detect file {}",
                fixture_name,
                expected_file
            );
            assert_eq!(discovery.has_go_mod, exp_go, "Fixture {} go_mod mismatch", fixture_name);
            assert_eq!(discovery.has_package_json, exp_node, "Fixture {} package_json mismatch", fixture_name);
            assert_eq!(discovery.has_pyproject_toml, exp_py, "Fixture {} pyproject mismatch", fixture_name);
            assert_eq!(discovery.has_cargo_toml, exp_rust, "Fixture {} cargo_toml mismatch", fixture_name);
            assert_eq!(discovery.has_docker_compose(), exp_compose, "Fixture {} docker_compose mismatch", fixture_name);
        }
    }

    #[test]
    fn test_workload_classifier() {
        let discovery = ManifestParser::discover_in_dir(Path::new("."));
        let classification = WorkloadClassifier::classify(&discovery, "python_vllm_gpu");
        assert_eq!(classification.workload_kind, WorkloadKind::PythonVllmGpu);
    }

    #[test]
    fn test_architecture_planner() {
        let discovery = ManifestParser::discover_in_dir(Path::new("."));
        let classification = WorkloadClassifier::classify(&discovery, "node_app");
        let plans = ArchitecturePlanner::generate_plans(&classification);

        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
    }

    #[test]
    fn test_missing_artifact_generator() {
        let discovery = ManifestParser::discover_in_dir(Path::new("."));
        let classification = WorkloadClassifier::classify(&discovery, "node_app");
        let artifacts = MissingArtifactGenerator::generate_missing_artifacts(&classification, false);

        assert!(!artifacts.is_empty());
        assert_eq!(artifacts[0].file_path, "Dockerfile");
    }

    #[test]
    fn test_ast_skeletonizer() {
        use parsers::CodebaseSkeletonizer;
        let skeleton = CodebaseSkeletonizer::scan_directory(Path::new(".")).unwrap();
        assert!(skeleton.total_files_scanned > 0);
        assert!(skeleton.token_compression_ratio >= 0.90);
    }

    #[test]
    fn test_llm_system_architect_feedback_loop() {
        use architect::LLMSystemArchitect;
        let architect = LLMSystemArchitect::new("DeepSeek", "deepseek-r1");
        let plan = architect.synthesize_architecture("test-service", "SKELETON_IR");
        assert_eq!(plan.nodes.len(), 2);

        let (refined, receipt) = architect.refine_plan_on_feedback(plan, "UNSAT: VRAM constraint exceeded");
        assert!(receipt.adjusted);
        assert!(refined.estimated_monthly_cost_usd > 42.0);
    }

    // =========================================================================
    // Workload Classification Tests: All 7 Archetypes
    // =========================================================================

    #[test]
    fn test_classify_all_archetypes_by_directory_name() {
        let empty_discovery = parsers::ManifestDiscovery::default();

        // Archetype 1: PythonVllmGpu
        let c1 = WorkloadClassifier::classify(&empty_discovery, "python_vllm_gpu");
        assert_eq!(c1.workload_kind, WorkloadKind::PythonVllmGpu);
        assert_eq!(c1.recommended_ram_mb, 32768);
        assert_eq!(c1.recommended_cpu_cores, 8);
        assert_eq!(c1.recommended_vram_mb, 24576);
        assert_eq!(c1.recommended_gpu_count, 1);
        assert!(c1.description.contains("Python High-Performance"));

        // Archetype 2: StaticWeb
        let c2 = WorkloadClassifier::classify(&empty_discovery, "react_vite_static");
        assert_eq!(c2.workload_kind, WorkloadKind::StaticWeb);
        assert_eq!(c2.recommended_ram_mb, 512);
        assert_eq!(c2.recommended_cpu_cores, 1);
        assert_eq!(c2.recommended_vram_mb, 0);
        assert_eq!(c2.recommended_gpu_count, 0);
        assert!(c2.description.contains("Static Web Frontend"));

        // Archetype 3: RustWasm
        let c3 = WorkloadClassifier::classify(&empty_discovery, "rust_wasm_edge");
        assert_eq!(c3.workload_kind, WorkloadKind::RustWasm);
        assert_eq!(c3.recommended_ram_mb, 256);
        assert_eq!(c3.recommended_cpu_cores, 1);
        assert_eq!(c3.recommended_vram_mb, 0);
        assert_eq!(c3.recommended_gpu_count, 0);
        assert!(c3.description.contains("Rust WebAssembly"));

        // Archetype 4: GoWorker
        let c4 = WorkloadClassifier::classify(&empty_discovery, "go_worker");
        assert_eq!(c4.workload_kind, WorkloadKind::GoWorker);
        assert_eq!(c4.recommended_ram_mb, 1024);
        assert_eq!(c4.recommended_cpu_cores, 2);
        assert_eq!(c4.recommended_vram_mb, 0);
        assert_eq!(c4.recommended_gpu_count, 0);
        assert!(c4.description.contains("Go Distributed"));

        // Archetype 5: StatefulStore
        let c5 = WorkloadClassifier::classify(&empty_discovery, "stateful_store_db");
        assert_eq!(c5.workload_kind, WorkloadKind::StatefulStore);
        assert_eq!(c5.recommended_ram_mb, 8192);
        assert_eq!(c5.recommended_cpu_cores, 4);
        assert_eq!(c5.recommended_vram_mb, 0);
        assert_eq!(c5.recommended_gpu_count, 0);
        assert!(c5.description.contains("Stateful In-Memory"));

        // Archetype 6: NodePostgresApi
        let c6 = WorkloadClassifier::classify(&empty_discovery, "node_postgres_api");
        assert_eq!(c6.workload_kind, WorkloadKind::NodePostgresApi);
        assert_eq!(c6.recommended_ram_mb, 2048);
        assert_eq!(c6.recommended_cpu_cores, 2);
        assert_eq!(c6.recommended_vram_mb, 0);
        assert_eq!(c6.recommended_gpu_count, 0);
        assert!(c6.description.contains("Node.js REST API"));

        // Archetype 7: GenericContainer
        let c7 = WorkloadClassifier::classify(&empty_discovery, "custom_service");
        assert_eq!(c7.workload_kind, WorkloadKind::GenericContainer);
        assert_eq!(c7.recommended_ram_mb, 1024);
        assert_eq!(c7.recommended_cpu_cores, 1);
        assert_eq!(c7.recommended_vram_mb, 0);
        assert_eq!(c7.recommended_gpu_count, 0);
        assert!(c7.description.contains("Generic Containerized"));
    }

    #[test]
    fn test_classify_by_manifest_discovery() {
        let base_discovery = parsers::ManifestDiscovery::default();

        // Manifest has pyproject.toml -> PythonVllmGpu
        let mut py_disc = base_discovery.clone();
        py_disc.has_pyproject_toml = true;
        let c_py = WorkloadClassifier::classify(&py_disc, "backend_pipeline");
        assert_eq!(c_py.workload_kind, WorkloadKind::PythonVllmGpu);

        // Manifest has go.mod -> GoWorker
        let mut go_disc = base_discovery.clone();
        go_disc.has_go_mod = true;
        let c_go = WorkloadClassifier::classify(&go_disc, "backend_pipeline");
        assert_eq!(c_go.workload_kind, WorkloadKind::GoWorker);

        // Manifest has package.json -> NodePostgresApi
        let mut node_disc = base_discovery.clone();
        node_disc.has_package_json = true;
        let c_node = WorkloadClassifier::classify(&node_disc, "backend_pipeline");
        assert_eq!(c_node.workload_kind, WorkloadKind::NodePostgresApi);

        // Manifest has cargo.toml and wasm in name -> RustWasm
        let mut rust_disc = base_discovery.clone();
        rust_disc.has_cargo_toml = true;
        let c_rust = WorkloadClassifier::classify(&rust_disc, "edge_wasm_filter");
        assert_eq!(c_rust.workload_kind, WorkloadKind::RustWasm);
    }

    #[test]
    fn test_classify_fixtures_against_all_archetypes() {
        let fixtures = fixtures_base_dir();

        let test_cases = [
            ("python_vllm_gpu", WorkloadKind::PythonVllmGpu),
            ("react_vite_static", WorkloadKind::StaticWeb),
            ("rust_wasm", WorkloadKind::RustWasm),
            ("go_worker", WorkloadKind::GoWorker),
            ("stateful_store", WorkloadKind::StatefulStore),
            ("node_postgres_api", WorkloadKind::NodePostgresApi),
        ];

        for (dir_name, expected_kind) in test_cases {
            let fixture_path = fixtures.join(dir_name);
            assert!(fixture_path.exists(), "Fixture path {:?} must exist", fixture_path);

            let discovery = ManifestParser::discover_in_dir(&fixture_path);
            let classification = WorkloadClassifier::classify(&discovery, dir_name);

            assert_eq!(
                classification.workload_kind, expected_kind,
                "Fixture {} did not classify as expected {:?}",
                dir_name, expected_kind
            );
        }
    }

    #[test]
    fn test_classify_edge_cases_and_case_insensitivity() {
        let empty_discovery = parsers::ManifestDiscovery::default();

        // Case insensitivity
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "PYTHON_VLLM_GPU").workload_kind,
            WorkloadKind::PythonVllmGpu
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "Vite-Frontend").workload_kind,
            WorkloadKind::StaticWeb
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "Rust_WASM_Plugin").workload_kind,
            WorkloadKind::RustWasm
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "DISTRIBUTED_WORKER").workload_kind,
            WorkloadKind::GoWorker
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "Redis_Store_Db").workload_kind,
            WorkloadKind::StatefulStore
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "EXPRESS_NODE_SERVICE").workload_kind,
            WorkloadKind::NodePostgresApi
        );
        assert_eq!(
            WorkloadClassifier::classify(&empty_discovery, "MY_ARBITRARY_DAEMON").workload_kind,
            WorkloadKind::GenericContainer
        );
    }

    // =========================================================================
    // 3-Tier Architecture Synthesis Tests: All Archetypes & Cost Modeling
    // =========================================================================

    #[test]
    fn test_plan_synthesis_python_vllm_gpu() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "vllm_llm_inference");
        assert_eq!(classification.workload_kind, WorkloadKind::PythonVllmGpu);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Tier 1: Cheapest
        let cheapest = &plans[0];
        assert_eq!(cheapest.tier, PlanTier::Cheapest);
        assert_eq!(cheapest.provider, "LOCAL_DOCKER");
        assert_eq!(cheapest.estimated_monthly_cost_usd, 0.0);
        assert_eq!(cheapest.nodes.len(), 1);
        assert_eq!(cheapest.nodes[0].image, "vllm/vllm-openai:latest");
        assert_eq!(cheapest.nodes[0].role, "gpu-worker");
        assert!(cheapest.nodes[0].ports.contains(&"8000:8000".to_string()));

        // Tier 2: Balanced
        let balanced = &plans[1];
        assert_eq!(balanced.tier, PlanTier::Balanced);
        assert_eq!(balanced.provider, "AWS");
        assert!(balanced.estimated_monthly_cost_usd > 0.0);
        assert_eq!(balanced.nodes.len(), 2);
        assert!(balanced.nodes.iter().any(|n| n.role == "gpu-worker"));
        assert!(balanced.nodes.iter().any(|n| n.role == "load-balancer"));

        // Tier 3: High-Performance
        let hp = &plans[2];
        assert_eq!(hp.tier, PlanTier::HighPerformance);
        assert_eq!(hp.provider, "GCP_GPU_CLUSTER");
        assert!(hp.estimated_monthly_cost_usd > balanced.estimated_monthly_cost_usd);
        assert_eq!(hp.nodes.len(), 3);
        assert!(hp.nodes.iter().any(|n| n.role == "gpu-worker-primary"));
        assert!(hp.nodes.iter().any(|n| n.role == "gpu-worker-secondary"));
        assert!(hp.nodes.iter().any(|n| n.role == "load-balancer"));

        // Cost progression invariant
        assert!(cheapest.estimated_monthly_cost_usd < balanced.estimated_monthly_cost_usd);
        assert!(balanced.estimated_monthly_cost_usd < hp.estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_static_web() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "vite_static_site");
        assert_eq!(classification.workload_kind, WorkloadKind::StaticWeb);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Cheapest: Local Nginx
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_DOCKER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);
        assert_eq!(plans[0].nodes[0].image, "nginx:alpine");

        // Balanced: Cloudflare Pages Edge
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "CLOUDFLARE_PAGES");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);
        assert_eq!(plans[1].nodes[0].role, "edge-cdn");

        // HighPerformance: S3 + CloudFront Edge
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "AWS_CLOUDFRONT_S3");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);
        assert_eq!(plans[2].nodes.len(), 2);
        assert!(plans[2].nodes.iter().any(|n| n.role == "edge-cache"));
        assert!(plans[2].nodes.iter().any(|n| n.role == "origin-storage"));

        // Cost monotonicity
        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_rust_wasm() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "rust_wasm_proxy");
        assert_eq!(classification.workload_kind, WorkloadKind::RustWasm);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Cheapest: Wasmer local runtime
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_WASMER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);
        assert_eq!(plans[0].nodes[0].image, "wasmer/runtime:latest");

        // Balanced: Fastly Compute edge
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "FASTLY_COMPUTE");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);
        assert_eq!(plans[1].nodes[0].role, "edge-compute");

        // HighPerformance: Cloudflare Workers edge mesh with KV
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "CLOUDFLARE_WORKERS");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);
        assert_eq!(plans[2].nodes.len(), 2);
        assert!(plans[2].nodes.iter().any(|n| n.role == "edge-storage"));

        // Cost monotonicity
        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_go_worker() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "go_event_worker");
        assert_eq!(classification.workload_kind, WorkloadKind::GoWorker);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Cheapest: Local Go worker
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_DOCKER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);

        // Balanced: DigitalOcean with Redis broker
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "DIGITAL_OCEAN");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);
        assert_eq!(plans[1].nodes.len(), 2);
        assert!(plans[1].nodes.iter().any(|n| n.role == "queue"));

        // HighPerformance: AWS ECS Fargate auto-scaling worker fleet with SQS
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "AWS_ECS_FARGATE");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);
        assert_eq!(plans[2].nodes.len(), 2);
        assert!(plans[2].nodes.iter().any(|n| n.role == "auto-scaling-workers"));
        assert!(plans[2].nodes.iter().any(|n| n.role == "managed-queue"));

        // Cost monotonicity
        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_stateful_store() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "stateful_postgres_db");
        assert_eq!(classification.workload_kind, WorkloadKind::StatefulStore);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Cheapest: Local Postgres
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_DOCKER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);
        assert_eq!(plans[0].nodes[0].role, "primary-store");

        // Balanced: Multi-AZ AWS RDS with Read Replica
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "AWS_RDS");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);
        assert_eq!(plans[1].nodes.len(), 2);
        assert!(plans[1].nodes.iter().any(|n| n.role == "managed-db-primary"));
        assert!(plans[1].nodes.iter().any(|n| n.role == "read-replica"));

        // HighPerformance: Distributed GCP Spanner consensus cluster
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "GCP_SPANNER");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);
        assert_eq!(plans[2].nodes.len(), 2);
        assert!(plans[2].nodes.iter().any(|n| n.role == "distributed-consensus"));
        assert!(plans[2].nodes.iter().any(|n| n.role == "consensus-witness"));

        // Cost monotonicity
        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_node_postgres_api() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "node_postgres_api");
        assert_eq!(classification.workload_kind, WorkloadKind::NodePostgresApi);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        // Cheapest: Local Docker
        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_DOCKER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);

        // Balanced: AWS 2-tier (app + postgres)
        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "AWS");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);
        assert_eq!(plans[1].nodes.len(), 2);

        // HighPerformance: AZURE_AKS cluster with Azure Postgres Flexible + App Gateway
        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "AZURE_AKS");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);
        assert_eq!(plans[2].nodes.len(), 3);
        assert!(plans[2].nodes.iter().any(|n| n.role == "api-server-pool"));
        assert!(plans[2].nodes.iter().any(|n| n.role == "azure-postgres-flexible"));
        assert!(plans[2].nodes.iter().any(|n| n.role == "application-gateway-waf"));

        // Cost monotonicity
        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_plan_synthesis_generic_container() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "misc_service");
        assert_eq!(classification.workload_kind, WorkloadKind::GenericContainer);

        let plans = ArchitecturePlanner::generate_plans(&classification);
        assert_eq!(plans.len(), 3);

        assert_eq!(plans[0].tier, PlanTier::Cheapest);
        assert_eq!(plans[0].provider, "LOCAL_DOCKER");
        assert_eq!(plans[0].estimated_monthly_cost_usd, 0.0);

        assert_eq!(plans[1].tier, PlanTier::Balanced);
        assert_eq!(plans[1].provider, "AWS");
        assert!(plans[1].estimated_monthly_cost_usd > 0.0);

        assert_eq!(plans[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans[2].provider, "GCP_GPU_CLUSTER");
        assert!(plans[2].estimated_monthly_cost_usd > plans[1].estimated_monthly_cost_usd);

        assert!(plans[0].estimated_monthly_cost_usd < plans[1].estimated_monthly_cost_usd);
        assert!(plans[1].estimated_monthly_cost_usd < plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_3tier_cost_modeling_invariants_across_all_archetypes() {
        let empty_discovery = parsers::ManifestDiscovery::default();

        let archetype_names = [
            "python_vllm_gpu",
            "react_vite_static",
            "rust_wasm",
            "go_worker",
            "stateful_store",
            "node_postgres_api",
            "generic_container",
        ];

        for dir_name in archetype_names {
            let classification = WorkloadClassifier::classify(&empty_discovery, dir_name);
            let plans = ArchitecturePlanner::generate_plans(&classification);

            // Invariant 1: Exactly 3 tiers synthesized
            assert_eq!(plans.len(), 3, "Archetype {} must have 3 plans", dir_name);

            // Invariant 2: Tier sequence strictly Cheapest -> Balanced -> HighPerformance
            assert_eq!(plans[0].tier, PlanTier::Cheapest);
            assert_eq!(plans[1].tier, PlanTier::Balanced);
            assert_eq!(plans[2].tier, PlanTier::HighPerformance);

            // Invariant 3: Cost monotonicity (Cheapest <= Balanced <= HighPerformance)
            assert!(
                plans[0].estimated_monthly_cost_usd <= plans[1].estimated_monthly_cost_usd,
                "Archetype {} Cheapest cost ({}) should be <= Balanced cost ({})",
                dir_name, plans[0].estimated_monthly_cost_usd, plans[1].estimated_monthly_cost_usd
            );
            assert!(
                plans[1].estimated_monthly_cost_usd <= plans[2].estimated_monthly_cost_usd,
                "Archetype {} Balanced cost ({}) should be <= HighPerformance cost ({})",
                dir_name, plans[1].estimated_monthly_cost_usd, plans[2].estimated_monthly_cost_usd
            );

            // Invariant 4: Non-negative costs
            for plan in &plans {
                assert!(plan.estimated_monthly_cost_usd >= 0.0);
            }

            // Invariant 5: Provider selection validity
            for plan in &plans {
                assert!(!plan.provider.is_empty());
                assert!(!plan.description.is_empty());
                assert!(plan.description.contains(&classification.description));
            }

            // Invariant 6: Topology completeness
            for plan in &plans {
                assert!(!plan.nodes.is_empty());
                for node in &plan.nodes {
                    assert!(!node.name.is_empty());
                    assert!(!node.role.is_empty());
                    assert!(!node.image.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_provider_selection_edge_vs_cloud_matrix() {
        let empty_discovery = parsers::ManifestDiscovery::default();

        // Edge static frontend selects edge CDN providers
        let static_class = WorkloadClassifier::classify(&empty_discovery, "react_app");
        let static_plans = ArchitecturePlanner::generate_plans(&static_class);
        assert_eq!(static_plans[1].provider, "CLOUDFLARE_PAGES");
        assert_eq!(static_plans[2].provider, "AWS_CLOUDFRONT_S3");

        // Edge Wasm selects serverless edge compute providers
        let wasm_class = WorkloadClassifier::classify(&empty_discovery, "wasm_service");
        let wasm_plans = ArchitecturePlanner::generate_plans(&wasm_class);
        assert_eq!(wasm_plans[0].provider, "LOCAL_WASMER");
        assert_eq!(wasm_plans[1].provider, "FASTLY_COMPUTE");
        assert_eq!(wasm_plans[2].provider, "CLOUDFLARE_WORKERS");

        // Stateful DB selects managed persistent database providers
        let store_class = WorkloadClassifier::classify(&empty_discovery, "db_store");
        let store_plans = ArchitecturePlanner::generate_plans(&store_class);
        assert_eq!(store_plans[1].provider, "AWS_RDS");
        assert_eq!(store_plans[2].provider, "GCP_SPANNER");

        // LLM GPU selects GPU cluster providers
        let gpu_class = WorkloadClassifier::classify(&empty_discovery, "vllm_gpu");
        let gpu_plans = ArchitecturePlanner::generate_plans(&gpu_class);
        assert_eq!(gpu_plans[1].provider, "AWS");
        assert_eq!(gpu_plans[2].provider, "GCP_GPU_CLUSTER");
    }

    #[test]
    fn test_serialization_roundtrip_plans_and_classifications() {
        let empty_discovery = parsers::ManifestDiscovery::default();
        let classification = WorkloadClassifier::classify(&empty_discovery, "node_postgres_api");
        let plans = ArchitecturePlanner::generate_plans(&classification);

        // Roundtrip classification
        let class_json = serde_json::to_string_pretty(&classification).expect("serialize classification");
        let class_deser: classifier::ProjectClassification = serde_json::from_str(&class_json).expect("deserialize classification");
        assert_eq!(class_deser.workload_kind, classification.workload_kind);
        assert_eq!(class_deser.recommended_ram_mb, classification.recommended_ram_mb);

        // Roundtrip plans
        let plans_json = serde_json::to_string_pretty(&plans).expect("serialize plans");
        let plans_deser: Vec<planner::ArchitecturePlan> = serde_json::from_str(&plans_json).expect("deserialize plans");
        assert_eq!(plans_deser.len(), 3);
        assert_eq!(plans_deser[0].tier, PlanTier::Cheapest);
        assert_eq!(plans_deser[1].tier, PlanTier::Balanced);
        assert_eq!(plans_deser[2].tier, PlanTier::HighPerformance);
        assert_eq!(plans_deser[1].nodes.len(), plans[1].nodes.len());
        assert_eq!(plans_deser[2].estimated_monthly_cost_usd, plans[2].estimated_monthly_cost_usd);
    }

    #[test]
    fn test_missing_artifact_generation_for_all_archetypes() {
        let empty_discovery = parsers::ManifestDiscovery::default();

        let archetype_names = [
            ("python_vllm_gpu", "vllm"),
            ("react_vite_static", "nginx"),
            ("rust_wasm", "wasm32-unknown-unknown"),
            ("go_worker", "golang"),
            ("stateful_store", "postgres"),
            ("node_postgres_api", "node"),
            ("generic_container", "alpine"),
        ];

        for (dir_name, expected_token) in archetype_names {
            let classification = WorkloadClassifier::classify(&empty_discovery, dir_name);
            let artifacts = MissingArtifactGenerator::generate_missing_artifacts(&classification, false);

            assert_eq!(artifacts.len(), 2, "Expected Dockerfile and docker-compose.yml for {}", dir_name);
            assert_eq!(artifacts[0].file_path, "Dockerfile");
            assert_eq!(artifacts[0].artifact_type, "DOCKERFILE");
            assert!(
                artifacts[0].content.contains(expected_token),
                "Dockerfile for {} should contain '{}', got:\n{}",
                dir_name, expected_token, artifacts[0].content
            );

            assert_eq!(artifacts[1].file_path, "docker-compose.yml");
            assert_eq!(artifacts[1].artifact_type, "DOCKER_COMPOSE");
        }

        // When Dockerfile already exists, only docker-compose.yml is generated
        let classification = WorkloadClassifier::classify(&empty_discovery, "react_vite_static");
        let artifacts = MissingArtifactGenerator::generate_missing_artifacts(&classification, true);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].file_path, "docker-compose.yml");
    }
}

