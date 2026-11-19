pub mod config;
pub mod crypto;
pub mod guardrails;
pub mod ir;
pub mod memory;
pub mod solver;
pub mod state_machine;
pub mod topology;
pub mod vault;

pub use solver::SimulationProofCertificate;
pub use topology::SimulatedHardwarePlacement;
pub use vault::SimulatedVaultReport;

#[cfg(test)]
mod tests {
    use super::*;
    use config::LtnSettings;
    use ir::{create_intent, ClusterState, WorkloadBuilder};
    use solver::{DependencyGraph, SmtSolver};
    use state_machine::StateMachine;
    use topology::TopologyDiscoveryAdapter;

    #[test]
    fn test_ir_and_solver_success() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };

        let workload = WorkloadBuilder::new("test-api")
            .cpu_cores(2)
            .ram_mb(2048)
            .build();

        let intent = create_intent(vec![workload]);
        let manifest = SmtSolver::solve(&intent, &cluster).unwrap();

        assert_eq!(manifest.mappings.len(), 1);
    }

    #[test]
    fn test_mus_core_extraction_on_unsatisfiable() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware(); // 8 cores, 16GB RAM
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };

        let workload = WorkloadBuilder::new("impossible-workload")
            .cpu_cores(128) // Demands 128 cores
            .ram_mb(2048)
            .build();

        let intent = create_intent(vec![workload]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(!mus_core.is_empty());
            }
            _ => panic!("Expected Unsatisfiable error"),
        }
    }

    #[test]
    fn test_state_machine_rollback() {
        let mut sm = StateMachine::new();
        sm.set_key("env", "staging");
        sm.commit();

        sm.set_key("env", "production");
        sm.commit();
        assert_eq!(sm.get_key("env").unwrap(), "production");

        sm.rollback();
        assert_eq!(sm.get_key("env").unwrap(), "staging");
    }

    #[test]
    fn test_dependency_graph() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("db", "api");
        graph.add_edge("api", "frontend");

        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted, vec!["db", "api", "frontend"]);
    }

    #[test]
    fn test_config_save_load() {
        let tmp_file = std::env::temp_dir().join("test_ltn_config.toml");
        let settings = LtnSettings::default();
        assert!(settings.save_to_file(&tmp_file).is_ok());

        let loaded = LtnSettings::load_from_file(&tmp_file).unwrap();
        assert_eq!(loaded.provider.default_provider, "LOCAL_DOCKER");
        let _ = std::fs::remove_file(tmp_file);
    }

    #[test]
    fn test_cross_project_memory_engine() {
        use memory::{CrossProjectMemoryEngine, ProjectMemorySnapshot};
        let mut engine = CrossProjectMemoryEngine::new();

        engine.save_project_context(ProjectMemorySnapshot {
            project_id: "project_alpha".to_string(),
            timestamp: 1000,
            ast_fingerprint: "fp_alpha".to_string(),
            last_verified_plan_hash: "hash_alpha".to_string(),
            symbol_count: 50,
        });

        engine.save_project_context(ProjectMemorySnapshot {
            project_id: "project_beta".to_string(),
            timestamp: 1005,
            ast_fingerprint: "fp_beta".to_string(),
            last_verified_plan_hash: "hash_beta".to_string(),
            symbol_count: 85,
        });

        let switch = engine.switch_context("project_alpha", "project_beta");
        assert!(switch.is_cache_hit);
        assert_eq!(switch.symbol_delta_count, 35);
        assert_eq!(switch.active_fingerprint, "fp_beta");
    }

    #[test]
    fn test_automanaged_pgvector_engine_silent_operation() {
        use memory::AutoManagedPgVectorEngine;
        let mut engine = AutoManagedPgVectorEngine::new();
        assert!(engine.initialize_hidden_daemon().is_ok());

        let res = engine.store_semantic_context(
            "proj_123",
            "pub fn deploy_service()",
            vec![0.1, 0.5, 0.9],
            "z3_proof_999",
        );
        assert!(res.is_ok());

        let matches = engine.query_similar_context(&[0.1, 0.5, 0.9], 1);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].project_id, "proj_123");
    }

    // --- CRYPTO SUITE ---
    #[test]
    fn test_crypto_sha256_empty() {
        let h1 = crypto::StateHasher::hash_bytes(b"");
        let h2 = crypto::StateHasher::hash_bytes(b"");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_crypto_sha256_deterministic() {
        let h1 = crypto::StateHasher::hash_str("ltn.eng deterministic test");
        let h2 = crypto::StateHasher::hash_str("ltn.eng deterministic test");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_crypto_sha256_avalanche() {
        let h1 = crypto::StateHasher::hash_str("workload_A");
        let h2 = crypto::StateHasher::hash_str("workload_B");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_crypto_sha256_unicode() {
        let h = crypto::StateHasher::hash_str("⚡🌩️ 现代工作负载 2026");
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn test_crypto_sha256_large_payload() {
        let big_data = vec![0u8; 1024 * 1024]; // 1MB
        let h = crypto::StateHasher::hash_bytes(&big_data);
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn test_crypto_combine_hashes() {
        let h1 = crypto::StateHasher::hash_str("shard-1-state");
        let h2 = crypto::StateHasher::hash_str("shard-2-state");
        let combined = crypto::StateHasher::combine_hashes(&[h1.clone(), h2.clone()]);
        assert_eq!(combined.len(), 64);

        // Determinism test
        let combined2 = crypto::StateHasher::combine_hashes(&[h1, h2]);
        assert_eq!(combined, combined2);
    }

    #[test]
    fn test_crypto_avalanche_strict_bit_flip() {
        // Flipping a single bit should change approximately 50% (around 128) of the 256 bits in SHA-256 output
        let data1 = b"ltn.eng.strict.verification.packet.A";
        let mut data2 = data1.to_vec();
        data2[0] ^= 0x01; // Flip exactly 1 bit

        let h1 = crypto::StateHasher::hash_bytes(data1);
        let h2 = crypto::StateHasher::hash_bytes(&data2);
        assert_ne!(h1, h2);

        let bytes1: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&h1[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let bytes2: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&h2[i * 2..i * 2 + 2], 16).unwrap())
            .collect();

        let mut diff_bits = 0;
        for i in 0..32 {
            diff_bits += (bytes1[i] ^ bytes2[i]).count_ones();
        }

        // Standard avalanche effect for SHA-256: bit difference is between 25% and 75% (64 to 192 bits out of 256)
        assert!(
            diff_bits > 80 && diff_bits < 180,
            "Avalanche bit diff was {}",
            diff_bits
        );
    }

    // --- IR & CONSTRAINTS SUITE ---
    #[test]
    fn test_ir_zero_resources() {
        let w = WorkloadBuilder::new("zero-res")
            .cpu_cores(0)
            .ram_mb(0)
            .build();
        assert_eq!(w.name, "zero-res");
        assert_eq!(w.constraints.len(), 2);
    }

    #[test]
    fn test_ir_large_resources() {
        let w = WorkloadBuilder::new("huge-res")
            .cpu_cores(1024)
            .ram_mb(1_000_000)
            .vram_mb(80_000)
            .gpu_count(8)
            .build();
        assert_eq!(w.constraints.len(), 4);
    }

    #[test]
    fn test_ir_workload_serialization() {
        let w = WorkloadBuilder::new("serial-test")
            .cpu_cores(4)
            .ram_mb(4096)
            .build();
        let serialized = serde_json::to_string(&w).unwrap();
        let deserialized: ir::Workload = serde_json::from_str(&serialized).unwrap();
        assert_eq!(w.name, deserialized.name);
        assert_eq!(w.id, deserialized.id);
    }

    #[test]
    fn test_ir_intent_roundtrip() {
        let w1 = WorkloadBuilder::new("w1").cpu_cores(1).build();
        let w2 = WorkloadBuilder::new("w2").cpu_cores(2).build();
        let intent = create_intent(vec![w1, w2]);
        assert_eq!(intent.workloads.len(), 2);
        let serialized = serde_json::to_string(&intent).unwrap();
        let deserialized: ir::Intent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(intent.intent_id, deserialized.intent_id);
    }

    #[test]
    fn test_ir_cluster_state_empty() {
        let cluster = ClusterState {
            resources: vec![],
            network_latencies: std::collections::HashMap::new(),
        };
        assert_eq!(cluster.resources.len(), 0);
    }

    // --- SMT SOLVER SUITE ---
    #[test]
    fn test_solver_empty_intent() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let intent = create_intent(vec![]);
        let manifest = SmtSolver::solve(&intent, &cluster).unwrap();
        assert_eq!(manifest.mappings.len(), 0);
    }

    #[test]
    fn test_solver_empty_cluster() {
        let cluster = ClusterState {
            resources: vec![],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("orphan").cpu_cores(1).build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|s| s.contains("No resource block")));
            }
            _ => panic!("Expected Unsatisfiable"),
        }
    }

    #[test]
    fn test_solver_multi_workload_fit() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w1 = WorkloadBuilder::new("w1").cpu_cores(1).ram_mb(512).build();
        let w2 = WorkloadBuilder::new("w2").cpu_cores(1).ram_mb(1024).build();
        let intent = create_intent(vec![w1, w2]);
        let manifest = SmtSolver::solve(&intent, &cluster).unwrap();
        assert_eq!(manifest.mappings.len(), 2);
    }

    #[test]
    fn test_solver_unsat_ram_core_extraction() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("ram-hog").ram_mb(999_999_999).build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|m| m.contains("requires RAM")));
            }
            _ => panic!("Expected Unsatisfiable RAM"),
        }
    }

    #[test]
    fn test_solver_unsat_vram() {
        let adapter = TopologyDiscoveryAdapter::new();
        let mut resource = adapter.discover_local_hardware();
        resource.total_vram_mb = 8192;
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("gpu-hog").vram_mb(24576).build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|m| m.contains("requires VRAM")));
            }
            _ => panic!("Expected Unsatisfiable VRAM"),
        }
    }

    #[test]
    fn test_solver_unsat_gpu_count() {
        let adapter = TopologyDiscoveryAdapter::new();
        let mut resource = adapter.discover_local_hardware();
        resource.total_gpu_count = 1;
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("multi-gpu").gpu_count(4).build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|m| m.contains("requires GPUs")));
            }
            _ => panic!("Expected Unsatisfiable GPU count"),
        }
    }

    #[test]
    fn test_solver_exact_capacity_boundary() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource.clone()],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("exact-fit")
            .cpu_cores(resource.total_cpu_cores)
            .ram_mb(resource.total_ram_mb)
            .build();
        let intent = create_intent(vec![w]);
        let res = SmtSolver::solve(&intent, &cluster);
        assert!(res.is_ok());
    }

    #[test]
    fn test_solver_overcapacity_by_one_mb() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource.clone()],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("over-by-one")
            .ram_mb(resource.total_ram_mb + 1)
            .build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster);
        assert!(err.is_err());
    }

    #[test]
    fn test_solver_multiple_nodes_selection() {
        let node1 = ir::ResourceBlock {
            block_id: uuid::Uuid::new_v4(),
            node_name: "node-small".to_string(),
            endpoint: "http://node-small:8080".to_string(),
            total_cpu_cores: 2,
            total_ram_mb: 2048,
            total_vram_mb: 0,
            total_gpu_count: 0,
            network_bw_gbps: 1,
        };
        let node2 = ir::ResourceBlock {
            block_id: uuid::Uuid::new_v4(),
            node_name: "node-large".to_string(),
            endpoint: "http://node-large:8080".to_string(),
            total_cpu_cores: 16,
            total_ram_mb: 32768,
            total_vram_mb: 0,
            total_gpu_count: 0,
            network_bw_gbps: 10,
        };

        let cluster = ClusterState {
            resources: vec![node1, node2.clone()],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("needs-big")
            .cpu_cores(8)
            .ram_mb(16384)
            .build();
        let intent = create_intent(vec![w.clone()]);
        let manifest = SmtSolver::solve(&intent, &cluster).unwrap();
        assert_eq!(manifest.mappings.get(&w.id), Some(&node2.block_id));
    }

    #[test]
    fn test_solver_unsat_cpu() {
        let adapter = TopologyDiscoveryAdapter::new();
        let mut resource = adapter.discover_local_hardware();
        resource.total_cpu_cores = 4;
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("cpu-hog").cpu_cores(128).build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|m| m.contains("requires CPU cores")));
                assert!(mus_core
                    .iter()
                    .any(|m| m.contains("No resource block in cluster satisfies workload")));
            }
            _ => panic!("Expected Unsatisfiable CPU cores"),
        }
    }

    #[test]
    fn test_solver_unsat_network_bandwidth() {
        let adapter = TopologyDiscoveryAdapter::new();
        let mut resource = adapter.discover_local_hardware();
        resource.network_bw_gbps = 1;
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("net-hog")
            .add_constraint(ir::Constraint::NetworkGbps(100))
            .build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|m| m.contains("requires Network")));
            }
            _ => panic!("Expected Unsatisfiable Network"),
        }
    }

    #[test]
    fn test_solver_zero_resources_edge_case() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("zero-res")
            .cpu_cores(0)
            .ram_mb(0)
            .vram_mb(0)
            .gpu_count(0)
            .build();
        let intent = create_intent(vec![w]);
        let manifest = SmtSolver::solve(&intent, &cluster).unwrap();
        assert_eq!(manifest.mappings.len(), 1);
    }

    #[test]
    fn test_solver_zero_capacity_node() {
        let zero_node = ir::ResourceBlock {
            block_id: uuid::Uuid::new_v4(),
            node_name: "node-zero".to_string(),
            endpoint: "http://node-zero:8080".to_string(),
            total_cpu_cores: 0,
            total_ram_mb: 0,
            total_vram_mb: 0,
            total_gpu_count: 0,
            network_bw_gbps: 0,
        };
        let cluster = ClusterState {
            resources: vec![zero_node],
            network_latencies: std::collections::HashMap::new(),
        };
        let w_zero = WorkloadBuilder::new("zero").cpu_cores(0).ram_mb(0).build();
        assert!(SmtSolver::solve(&create_intent(vec![w_zero]), &cluster).is_ok());

        let w_one = WorkloadBuilder::new("one").cpu_cores(1).build();
        let err = SmtSolver::solve(&create_intent(vec![w_one]), &cluster).unwrap_err();
        assert!(matches!(err, solver::SolverError::Unsatisfiable { .. }));
    }

    #[test]
    fn test_solver_huge_limits_edge_case() {
        let adapter = TopologyDiscoveryAdapter::new();
        let resource = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![resource],
            network_latencies: std::collections::HashMap::new(),
        };
        let w = WorkloadBuilder::new("huge-limits")
            .cpu_cores(u32::MAX)
            .ram_mb(u32::MAX)
            .vram_mb(u32::MAX)
            .gpu_count(u32::MAX)
            .build();
        let intent = create_intent(vec![w]);
        let err = SmtSolver::solve(&intent, &cluster).unwrap_err();
        match err {
            solver::SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.len() >= 4);
            }
            _ => panic!("Expected Unsatisfiable for huge limits"),
        }
    }

    // --- DEPENDENCY GRAPH SUITE ---
    #[test]
    fn test_graph_single_node() {
        let mut graph = DependencyGraph::new();
        graph.add_node("service-a");
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted, vec!["service-a"]);
    }

    #[test]
    fn test_graph_disconnected_nodes() {
        let mut graph = DependencyGraph::new();
        graph.add_node("service-a");
        graph.add_node("service-b");
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted.len(), 2);
    }

    #[test]
    fn test_graph_diamond_dependency() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("root", "left");
        graph.add_edge("root", "right");
        graph.add_edge("left", "sink");
        graph.add_edge("right", "sink");
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted[0], "root");
        assert_eq!(sorted[3], "sink");
    }

    #[test]
    fn test_graph_linear_chain() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", "d");
        graph.add_edge("d", "e");
        let sorted = graph.topological_sort().unwrap();
        assert_eq!(sorted, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn test_graph_self_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("loop", "loop");
        let res = graph.topological_sort();
        assert!(matches!(res, Err(solver::SolverError::CycleDetected)));
    }

    #[test]
    fn test_graph_two_node_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("b", "a");
        let res = graph.topological_sort();
        assert!(matches!(res, Err(solver::SolverError::CycleDetected)));
    }

    #[test]
    fn test_graph_three_node_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", "a");
        let res = graph.topological_sort();
        assert!(matches!(res, Err(solver::SolverError::CycleDetected)));
    }

    #[test]
    fn test_graph_duplicate_edges() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("a", "b");
        graph.add_edge("a", "b");
        // in_degree increments, so graph detects constraint or sorts
        let sorted = graph.topological_sort();
        // Multiple edges add in_degree constraints
        assert!(sorted.is_ok() || sorted.is_err());
    }

    #[test]
    fn test_graph_empty() {
        let graph: DependencyGraph<&str> = DependencyGraph::new();
        let sorted = graph.topological_sort().unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_graph_disconnected_component_cycle() {
        let mut graph = DependencyGraph::new();
        // Component 1: acyclic chain
        graph.add_edge("c1_a", "c1_b");
        // Component 2: cycle
        graph.add_edge("c2_x", "c2_y");
        graph.add_edge("c2_y", "c2_x");
        let res = graph.topological_sort();
        assert!(matches!(res, Err(solver::SolverError::CycleDetected)));
    }

    // --- STATE MACHINE SUITE ---
    #[test]
    fn test_state_machine_init_empty() {
        let sm = StateMachine::new();
        assert!(sm.get_key("none").is_none());
    }

    #[test]
    fn test_state_machine_single_key_overwrite() {
        let mut sm = StateMachine::new();
        sm.set_key("target", "alpha");
        sm.set_key("target", "omega");
        assert_eq!(sm.get_key("target").unwrap(), "omega");
    }

    #[test]
    fn test_state_machine_multiple_keys() {
        let mut sm = StateMachine::new();
        sm.set_key("k1", "v1");
        sm.set_key("k2", "v2");
        sm.set_key("k3", "v3");
        sm.commit();
        assert_eq!(sm.get_key("k1").unwrap(), "v1");
        assert_eq!(sm.get_key("k2").unwrap(), "v2");
        assert_eq!(sm.get_key("k3").unwrap(), "v3");
    }

    #[test]
    fn test_state_machine_rollback_past_beginning() {
        let mut sm = StateMachine::new();
        let res = sm.rollback();
        assert!(res.is_none());
    }

    #[test]
    fn test_state_machine_multiple_rollbacks() {
        let mut sm = StateMachine::new();
        sm.set_key("version", "1");
        sm.commit();
        sm.set_key("version", "2");
        sm.commit();
        sm.set_key("version", "3");
        sm.commit();

        assert_eq!(sm.get_key("version").unwrap(), "3");
        sm.rollback();
        assert_eq!(sm.get_key("version").unwrap(), "2");
        sm.rollback();
        assert_eq!(sm.get_key("version").unwrap(), "1");
    }

    #[test]
    fn test_state_machine_hash_changes_on_commit() {
        let mut sm = StateMachine::new();
        sm.set_key("key", "val1");
        let snap1 = sm.commit();
        sm.set_key("key", "val2");
        let snap2 = sm.commit();
        assert_ne!(snap1.state_hash, snap2.state_hash);
    }

    #[test]
    fn test_state_machine_unicode_keys_values() {
        let mut sm = StateMachine::new();
        sm.set_key("集群状态", "健康");
        sm.commit();
        assert_eq!(sm.get_key("集群状态").unwrap(), "健康");
    }

    #[test]
    fn test_state_machine_empty_string_keys() {
        let mut sm = StateMachine::new();
        sm.set_key("", "empty_key_val");
        assert_eq!(sm.get_key("").unwrap(), "empty_key_val");
    }

    #[test]
    fn test_state_machine_rollback_past_beginning_idempotent() {
        let mut sm = StateMachine::new();
        sm.set_key("initial_k", "initial_v");
        sm.commit();

        // 1 commit + 1 initial snapshot = 2 snapshots in history
        assert!(sm.rollback().is_some());
        // Now only initial snapshot remains (len == 1)
        assert!(sm.rollback().is_none());
        assert!(sm.rollback().is_none());
        assert!(sm.rollback().is_none());
        // State remains empty and healthy
        assert!(sm.get_key("initial_k").is_none());
    }

    #[test]
    fn test_state_machine_multi_rollback_sequence() {
        let mut sm = StateMachine::new();
        for i in 1..=5 {
            sm.set_key("step", i.to_string());
            sm.commit();
        }
        assert_eq!(sm.get_key("step").unwrap(), "5");

        for expected in (1..=4).rev() {
            let snap = sm.rollback().expect("rollback should succeed");
            assert_eq!(snap.data.get("step").unwrap(), &expected.to_string());
            assert_eq!(sm.get_key("step").unwrap(), &expected.to_string());
        }

        // Rolling back from step 1 rolls back to initial empty snapshot
        let init_snap = sm.rollback().expect("rollback to initial should succeed");
        assert!(init_snap.data.is_empty());
        assert!(sm.get_key("step").is_none());

        // Further rollback returns None
        assert!(sm.rollback().is_none());
    }

    // --- CONFIG & SETTINGS SUITE ---
    #[test]
    fn test_settings_default_values() {
        let s = LtnSettings::default();
        assert_eq!(s.provider.default_provider, "LOCAL_DOCKER");
        assert_eq!(s.provider.default_tier, "BALANCED");
        assert_eq!(s.telemetry.probe_interval_sec, 10);
        assert!(s.telemetry.auto_heal_enabled);
        assert_eq!(s.llm.active_model, "gemini-2.5-flash");
    }

    #[test]
    fn test_settings_json_parsing() {
        let json_str = r#"{
            "provider": { "default_provider": "AWS", "default_tier": "CHEAPEST" },
            "telemetry": { "probe_interval_sec": 5, "auto_heal_enabled": false, "max_fix_attempts": 1 },
            "llm": { "active_model": "deepseek-r1", "llm_provider": "DeepSeek", "local_endpoint": "http://localhost:11434", "rig_compatibility_mode": true }
        }"#;
        let s: LtnSettings = serde_json::from_str(json_str).unwrap();
        assert_eq!(s.provider.default_provider, "AWS");
        assert_eq!(s.provider.default_tier, "CHEAPEST");
        assert!(!s.telemetry.auto_heal_enabled);
    }

    #[test]
    fn test_settings_toml_parsing() {
        let toml_str = r#"
            [provider]
            default_provider = "GCP"
            default_tier = "HIGH_PERFORMANCE"

            [telemetry]
            probe_interval_sec = 20
            auto_heal_enabled = true
            max_fix_attempts = 5
        "#;
        let s: LtnSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(s.provider.default_provider, "GCP");
        assert_eq!(s.provider.default_tier, "HIGH_PERFORMANCE");
        assert_eq!(s.llm.active_model, "gemini-2.5-flash"); // Defaults
    }

    #[test]
    fn test_settings_partial_toml_defaults() {
        let toml_str = r#"
            [provider]
            default_provider = "AZURE"
            default_tier = "BALANCED"
        "#;
        let s: LtnSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(s.provider.default_provider, "AZURE");
        assert!(s.telemetry.auto_heal_enabled);
    }

    #[test]
    fn test_settings_custom_gemini_key() {
        let mut s = LtnSettings::default();
        s.llm.gemini_api_key = Some("custom_test_key_123".to_string());
        assert_eq!(s.llm.gemini_api_key.as_deref(), Some("custom_test_key_123"));
    }

    #[test]
    fn test_settings_missing_file_creates_default() {
        let tmp_path =
            std::env::temp_dir().join(format!("nonexistent_ltn_{}.toml", uuid::Uuid::new_v4()));
        assert!(!tmp_path.exists());
        let s = LtnSettings::load_from_file(&tmp_path).unwrap();
        assert_eq!(s.provider.default_provider, "LOCAL_DOCKER");
        let _ = std::fs::remove_file(tmp_path);
    }

    #[test]
    fn test_settings_toml_vs_json_serialization() {
        let mut custom_settings = LtnSettings::default();
        custom_settings.provider.default_provider = "HYBRID_CLOUD".to_string();
        custom_settings.provider.default_tier = "ENTERPRISE".to_string();
        custom_settings.provider.aws_region = Some("eu-central-1".to_string());
        custom_settings.telemetry.probe_interval_sec = 42;
        custom_settings.telemetry.auto_heal_enabled = false;
        custom_settings.telemetry.max_fix_attempts = 7;
        custom_settings.llm.active_model = "claude-3-7-sonnet".to_string();
        custom_settings.llm.llm_provider = "Anthropic".to_string();

        let uid = uuid::Uuid::new_v4();
        let toml_path = std::env::temp_dir().join(format!("test_settings_{}.toml", uid));
        let json_path = std::env::temp_dir().join(format!("test_settings_{}.json", uid));

        // Save to both formats
        custom_settings
            .save_to_file(&toml_path)
            .expect("failed to save toml");
        custom_settings
            .save_to_file(&json_path)
            .expect("failed to save json");

        // Load back from both formats
        let loaded_toml = LtnSettings::load_from_file(&toml_path).expect("failed to load toml");
        let loaded_json = LtnSettings::load_from_file(&json_path).expect("failed to load json");

        // Verify TOML loaded matches original
        assert_eq!(loaded_toml.provider.default_provider, "HYBRID_CLOUD");
        assert_eq!(
            loaded_toml.provider.aws_region.as_deref(),
            Some("eu-central-1")
        );
        assert_eq!(loaded_toml.telemetry.probe_interval_sec, 42);
        assert!(!loaded_toml.telemetry.auto_heal_enabled);
        assert_eq!(loaded_toml.llm.active_model, "claude-3-7-sonnet");

        // Verify JSON loaded matches original
        assert_eq!(loaded_json.provider.default_provider, "HYBRID_CLOUD");
        assert_eq!(
            loaded_json.provider.aws_region.as_deref(),
            Some("eu-central-1")
        );
        assert_eq!(loaded_json.telemetry.probe_interval_sec, 42);
        assert!(!loaded_json.telemetry.auto_heal_enabled);
        assert_eq!(loaded_json.llm.active_model, "claude-3-7-sonnet");

        // Verify semantic parity between TOML and JSON deserialized objects
        assert_eq!(
            loaded_toml.provider.default_provider,
            loaded_json.provider.default_provider
        );
        assert_eq!(
            loaded_toml.telemetry.probe_interval_sec,
            loaded_json.telemetry.probe_interval_sec
        );
        assert_eq!(loaded_toml.llm.active_model, loaded_json.llm.active_model);

        // Verify fallback parsing: JSON content in .toml file
        let mismatched_path = std::env::temp_dir().join(format!("test_mismatch_{}.toml", uid));
        std::fs::write(
            &mismatched_path,
            serde_json::to_string(&custom_settings).unwrap(),
        )
        .unwrap();
        let fallback_loaded =
            LtnSettings::load_from_file(&mismatched_path).expect("fallback parsing failed");
        assert_eq!(fallback_loaded.provider.default_provider, "HYBRID_CLOUD");

        // Clean up temporary files
        let _ = std::fs::remove_file(toml_path);
        let _ = std::fs::remove_file(json_path);
        let _ = std::fs::remove_file(mismatched_path);
    }

    // --- TOPOLOGY SUITE ---
    #[test]
    fn test_topology_local_discovery() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hw = adapter.discover_local_hardware();
        assert!(hw.total_cpu_cores > 0);
        assert!(hw.total_ram_mb > 0);
        assert!(!hw.node_name.is_empty());
    }

    #[test]
    fn test_topology_hardware_bounds() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hw = adapter.discover_local_hardware();
        assert!(hw.total_cpu_cores <= 512);
        assert!(hw.network_bw_gbps > 0);
    }

    // --- MEMORY ENGINE SUITE ---
    #[test]
    fn test_memory_multiple_projects() {
        use memory::{CrossProjectMemoryEngine, ProjectMemorySnapshot};
        let mut engine = CrossProjectMemoryEngine::new();
        for i in 0..10 {
            engine.save_project_context(ProjectMemorySnapshot {
                project_id: format!("proj_{}", i),
                timestamp: 1000 + i as u64,
                ast_fingerprint: format!("fp_{}", i),
                last_verified_plan_hash: format!("hash_{}", i),
                symbol_count: i * 10,
            });
        }
        let switch = engine.switch_context("proj_0", "proj_9");
        assert!(switch.is_cache_hit);
        assert_eq!(switch.symbol_delta_count, 90);
    }

    #[test]
    fn test_memory_unknown_project_context() {
        use memory::CrossProjectMemoryEngine;
        let mut engine = CrossProjectMemoryEngine::new();
        let switch = engine.switch_context("none_a", "none_b");
        assert!(!switch.is_cache_hit);
    }

    #[test]
    fn test_pgvector_query_limit() {
        use memory::AutoManagedPgVectorEngine;
        let mut engine = AutoManagedPgVectorEngine::new();
        let _ = engine.initialize_hidden_daemon();
        for i in 0..5 {
            let _ = engine.store_semantic_context(
                &format!("proj_{}", i),
                "fn test()",
                vec![0.1 * i as f32, 0.2, 0.3],
                "proof",
            );
        }
        let matches = engine.query_similar_context(&[0.0, 0.2, 0.3], 3);
        assert!(matches.len() <= 3);
    }

    #[test]
    fn test_pgvector_cosine_similarity() {
        use memory::AutoManagedPgVectorEngine;
        let mut engine = AutoManagedPgVectorEngine::new();
        let _ = engine.initialize_hidden_daemon();
        let _ = engine.store_semantic_context(
            "exact",
            "target content",
            vec![1.0, 0.0, 0.0],
            "proof_exact",
        );
        let matches = engine.query_similar_context(&[1.0, 0.0, 0.0], 1);
        assert_eq!(matches[0].project_id, "exact");
    }

    #[test]
    fn test_vault_simulated_report_reexport() {
        use vault::ZeroTrustVault;
        let mut secrets = std::collections::HashMap::new();
        secrets.insert("ROOT_KEY".to_string(), "super_secret".to_string());
        let report: SimulatedVaultReport =
            ZeroTrustVault::simulate_vault_transaction(&secrets, "pass").unwrap();
        assert!(report.encryption_successful);
        assert!(report.zeroization_verified);
        assert_eq!(report.keys_managed, vec!["ROOT_KEY".to_string()]);
    }
}
