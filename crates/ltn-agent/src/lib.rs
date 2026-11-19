pub mod diagnostics;
pub mod gate;
pub mod harness;
pub mod prompt;
pub mod registry;
pub mod router;
pub mod scheduler;
pub mod simulator;
pub mod tickets;

pub use simulator::{DeterministicSimulationReport, DeterministicSimulator};

#[cfg(test)]
mod tests {
    use super::*;
    use diagnostics::CargoDiagnosticsParser;
    use gate::DeterministicGate;
    use harness::{AgentHarness, HarnessPhase};
    use ltn_core::ir::{
        create_intent, ClusterState, Constraint, ResourceBlock, WorkloadBuilder, WorkloadKind,
    };
    use ltn_core::solver::SolverError;
    use ltn_core::topology::TopologyDiscoveryAdapter;
    use registry::{AgentToolKind, ToolCall, ToolRegistry, ToolValidationError};
    use uuid::Uuid;

    // =========================================================================
    // 1. Sandboxed Agent Harness 5-Step Lifecycle Tests
    // (READ -> PLAN -> VALIDATE -> MUTATE -> VERIFY)
    // =========================================================================

    #[test]
    fn test_harness_lifecycle_success_all_5_steps() {
        let mut harness = AgentHarness::new();
        assert_eq!(harness.current_phase, HarnessPhase::Read);

        let results = harness.run_lifecycle("demo-app");

        // Exactly 5 steps executed in successful lifecycle
        assert_eq!(results.len(), 5);
        for res in &results {
            assert!(res.success, "Every lifecycle step must succeed");
        }

        // Verify exact 5-step phase progression
        assert_eq!(
            harness.phase_history,
            vec![
                HarnessPhase::Read,
                HarnessPhase::Plan,
                HarnessPhase::Validate,
                HarnessPhase::Mutate,
                HarnessPhase::Verify,
                HarnessPhase::Completed,
            ]
        );
        assert_eq!(harness.current_phase, HarnessPhase::Completed);

        // Verify state machine commit occurred at MUTATE phase
        assert_eq!(
            harness.state_machine.get_key("status"),
            Some(&"mutating".to_string())
        );
    }

    #[test]
    fn test_harness_lifecycle_validation_failure_triggers_rollback() {
        let mut harness = AgentHarness::new();

        // Cluster with limited resources (2 CPU cores, 1024 MB RAM)
        let cluster = ClusterState {
            resources: vec![ResourceBlock {
                block_id: Uuid::new_v4(),
                node_name: "constrained-node".to_string(),
                total_cpu_cores: 2,
                total_ram_mb: 1024,
                total_vram_mb: 0,
                total_gpu_count: 0,
                network_bw_gbps: 1,
                endpoint: "127.0.0.1:9090".to_string(),
            }],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload requiring 64 CPU cores (impossible for this cluster)
        let workload = WorkloadBuilder::new("impossible-app")
            .cpu_cores(64)
            .ram_mb(1024)
            .build();
        let intent = create_intent(vec![workload]);

        let results = harness.run_lifecycle_with_intent("impossible-app", intent, cluster);

        // Lifecycle must halt at step 3 and execute rollback: Read, Plan, Rollback
        assert_eq!(results.len(), 3);
        assert!(results[0].success); // Read
        assert!(results[1].success); // Plan
        assert!(results[2].success); // Rollback
        assert!(
            results[2].message.contains("rolled back"),
            "Last result should be state rollback"
        );

        // Phase history must end at Rollback, NOT proceed to Mutate or Verify
        assert_eq!(
            harness.phase_history,
            vec![
                HarnessPhase::Read,
                HarnessPhase::Plan,
                HarnessPhase::Validate,
                HarnessPhase::Rollback,
            ]
        );
        assert_eq!(harness.current_phase, HarnessPhase::Rollback);

        // State machine must NOT have been mutated
        assert_eq!(harness.state_machine.get_key("status"), None);
    }

    #[test]
    fn test_harness_lifecycle_state_machine_persistence() {
        let mut harness = AgentHarness::new();
        let initial_hash = harness.state_machine.current_hash();
        assert_eq!(harness.state_machine.get_key("status"), None);

        let _ = harness.run_lifecycle("audit-target");

        // Mutate phase commits changes to the state machine
        assert_eq!(
            harness.state_machine.get_key("status"),
            Some(&"mutating".to_string())
        );
        let mutated_hash = harness.state_machine.current_hash();
        assert_ne!(
            initial_hash, mutated_hash,
            "State hash must change after state machine mutation"
        );
    }

    // =========================================================================
    // 2. Tool Registry Type Checking, Schema Verification & Missing Parameters
    // =========================================================================

    #[test]
    fn test_registry_tool_definitions_schemas() {
        let definitions = ToolRegistry::all_tool_definitions();
        assert_eq!(definitions.len(), 6);

        let kinds: Vec<AgentToolKind> = definitions.iter().map(|d| d.kind.clone()).collect();
        assert!(kinds.contains(&AgentToolKind::Read));
        assert!(kinds.contains(&AgentToolKind::Plan));
        assert!(kinds.contains(&AgentToolKind::Validate));
        assert!(kinds.contains(&AgentToolKind::Mutate));
        assert!(kinds.contains(&AgentToolKind::Verify));
        assert!(kinds.contains(&AgentToolKind::Rollback));

        // Verify Mutate schema requires both target and payload
        let mutate_def = ToolRegistry::get_schema(&AgentToolKind::Mutate);
        let target_param = mutate_def
            .parameters
            .iter()
            .find(|p| p.name == "target")
            .unwrap();
        let payload_param = mutate_def
            .parameters
            .iter()
            .find(|p| p.name == "payload")
            .unwrap();
        assert!(target_param.required);
        assert_eq!(target_param.param_type, "string");
        assert!(payload_param.required);
        assert_eq!(payload_param.param_type, "string");

        // Verify Read schema requires target, but payload is optional
        let read_def = ToolRegistry::get_schema(&AgentToolKind::Read);
        let read_target = read_def
            .parameters
            .iter()
            .find(|p| p.name == "target")
            .unwrap();
        let read_payload = read_def
            .parameters
            .iter()
            .find(|p| p.name == "payload")
            .unwrap();
        assert!(read_target.required);
        assert!(!read_payload.required);
    }

    #[test]
    fn test_registry_type_checking_valid_json() {
        let raw_json = r#"{
            "tool": "mutate",
            "target": "auth-service",
            "payload": "update memory to 4096MB"
        }"#;

        let parsed = ToolRegistry::parse_and_validate_json(raw_json);
        assert!(parsed.is_ok());
        let call = parsed.unwrap();
        assert_eq!(call.tool, AgentToolKind::Mutate);
        assert_eq!(call.target, "auth-service");
        assert_eq!(call.payload, "update memory to 4096MB");
    }

    #[test]
    fn test_registry_type_checking_invalid_types() {
        // Non-string tool field
        let bad_tool = r#"{"tool": 12345, "target": "auth-service"}"#;
        let err = ToolRegistry::parse_and_validate_json(bad_tool).unwrap_err();
        assert!(matches!(err, ToolValidationError::TypeMismatch { field, .. } if field == "tool"));

        // Non-string target field
        let bad_target = r#"{"tool": "read", "target": true}"#;
        let err = ToolRegistry::parse_and_validate_json(bad_target).unwrap_err();
        assert!(
            matches!(err, ToolValidationError::TypeMismatch { field, .. } if field == "target")
        );

        // Non-string payload field
        let bad_payload = r#"{"tool": "read", "target": "project", "payload": ["item1", "item2"]}"#;
        let err = ToolRegistry::parse_and_validate_json(bad_payload).unwrap_err();
        assert!(
            matches!(err, ToolValidationError::TypeMismatch { field, .. } if field == "payload")
        );
    }

    #[test]
    fn test_registry_schema_verification_malformed_json() {
        // Syntax error
        let malformed = "{ not a valid json }";
        let err = ToolRegistry::parse_and_validate_json(malformed).unwrap_err();
        assert!(matches!(err, ToolValidationError::SchemaError(_)));

        // Non-object JSON
        let non_object = r#"["read", "target"]"#;
        let err = ToolRegistry::parse_and_validate_json(non_object).unwrap_err();
        assert!(matches!(err, ToolValidationError::SchemaError(_)));
    }

    #[test]
    fn test_registry_missing_parameter_handling_json() {
        // Missing "tool"
        let no_tool = r#"{"target": "demo"}"#;
        let err = ToolRegistry::parse_and_validate_json(no_tool).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::MissingParameter("tool".to_string())
        );

        // Missing "target"
        let no_target = r#"{"tool": "read"}"#;
        let err = ToolRegistry::parse_and_validate_json(no_target).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::MissingParameter("target".to_string())
        );

        // Empty whitespace "target"
        let empty_target = r#"{"tool": "read", "target": "   "}"#;
        let err = ToolRegistry::parse_and_validate_json(empty_target).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::MissingParameter("target".to_string())
        );

        // Missing required payload for Mutate
        let mutate_no_payload = r#"{"tool": "mutate", "target": "demo"}"#;
        let err = ToolRegistry::parse_and_validate_json(mutate_no_payload).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::MissingParameter("payload".to_string())
        );

        // Missing required payload for Rollback
        let rollback_no_payload = r#"{"tool": "rollback", "target": "demo", "payload": ""}"#;
        let err = ToolRegistry::parse_and_validate_json(rollback_no_payload).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::MissingParameter("payload".to_string())
        );
    }

    #[test]
    fn test_registry_missing_parameter_handling_execution() {
        // execute_tool with empty target must fail
        let call_empty_target = ToolCall {
            tool: AgentToolKind::Read,
            target: String::new(),
            payload: String::new(),
        };
        let res = ToolRegistry::execute_tool(&call_empty_target);
        assert!(!res.success);
        assert!(res.message.contains("Missing required parameter: target"));

        // execute_tool with Mutate and empty payload must fail
        let call_mutate_empty_payload = ToolCall {
            tool: AgentToolKind::Mutate,
            target: "project".to_string(),
            payload: String::new(),
        };
        let res = ToolRegistry::execute_tool(&call_mutate_empty_payload);
        assert!(!res.success);
        assert!(res.message.contains("Missing required parameter: payload"));

        // execute_tool with Rollback and empty payload must fail
        let call_rollback_empty_payload = ToolCall {
            tool: AgentToolKind::Rollback,
            target: "project".to_string(),
            payload: "   ".to_string(),
        };
        let res = ToolRegistry::execute_tool(&call_rollback_empty_payload);
        assert!(!res.success);
        assert!(res.message.contains("Missing required parameter: payload"));
    }

    #[test]
    fn test_registry_unknown_tool_rejection() {
        let unknown = r#"{"tool": "deploy_unverified", "target": "project"}"#;
        let err = ToolRegistry::parse_and_validate_json(unknown).unwrap_err();
        assert_eq!(
            err,
            ToolValidationError::UnknownTool("deploy_unverified".to_string())
        );
    }

    #[test]
    fn test_registry_execution_all_tool_kinds() {
        let kinds_and_payloads = [
            (AgentToolKind::Read, ""),
            (AgentToolKind::Plan, ""),
            (AgentToolKind::Validate, ""),
            (AgentToolKind::Mutate, "new config"),
            (AgentToolKind::Verify, ""),
            (AgentToolKind::Rollback, "reverting change"),
        ];

        for (kind, payload) in kinds_and_payloads {
            let call = ToolCall {
                tool: kind,
                target: "target-service".to_string(),
                payload: payload.to_string(),
            };
            let res = ToolRegistry::execute_tool(&call);
            assert!(res.success, "Tool execution failed for {:?}", call.tool);
            assert!(!res.message.is_empty());
            assert!(!res.output.is_empty());
        }
    }

    // =========================================================================
    // 3. Cargo Diagnostics Parsing (Warnings, Errors, Multi-file Spans)
    // =========================================================================

    #[test]
    fn test_harness_cargo_diagnostics_compiler_errors_and_warnings() {
        let error_json = r#"{"reason":"compiler-message","package_id":"api 0.1.0","target":{"name":"api"},"message":{"rendered":"error[E0425]: cannot find value `foo`","level":"error","message":"cannot find value `foo`","code":{"code":"E0425","explanation":"An undeclared label was used."},"spans":[{"file_name":"src/main.rs","line_start":12,"line_end":12,"column_start":5,"column_end":8,"is_primary":true,"label":"not found in this scope"}]}}"#;
        let warning_json = r#"{"reason":"compiler-message","package_id":"api 0.1.0","target":{"name":"api"},"message":{"rendered":"warning: unused variable `bar`","level":"warning","message":"unused variable `bar`","code":{"code":"unused_variables","explanation":null},"spans":[{"file_name":"src/lib.rs","line_start":4,"line_end":4,"column_start":9,"column_end":12,"is_primary":true,"label":"unused variable"}]}}"#;

        let stream = format!("{}\n{}", error_json, warning_json);
        let msgs = CargoDiagnosticsParser::parse_json_stream(&stream);

        assert_eq!(msgs.len(), 2);

        // Filter errors
        let errors = CargoDiagnosticsParser::filter_errors(&msgs);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].level.as_deref(), Some("error"));
        assert_eq!(errors[0].code.as_deref(), Some("E0425"));
        assert_eq!(errors[0].spans.len(), 1);
        assert_eq!(errors[0].spans[0].file_name, "src/main.rs");
        assert_eq!(errors[0].spans[0].line_start, 12);
        assert!(errors[0].spans[0].is_primary);

        // Filter warnings
        let warnings = CargoDiagnosticsParser::filter_warnings(&msgs);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].level.as_deref(), Some("warning"));
        assert_eq!(warnings[0].code.as_deref(), Some("unused_variables"));
        assert_eq!(warnings[0].spans[0].file_name, "src/lib.rs");

        // Error detection predicate
        assert!(CargoDiagnosticsParser::has_errors(&msgs));
    }

    #[test]
    fn test_harness_cargo_diagnostics_multi_file_spans() {
        let multi_file_json = r#"{
            "reason": "compiler-message",
            "package_id": "core 0.1.0",
            "target": {"name": "core"},
            "message": {
                "rendered": "error[E0308]: mismatched types",
                "level": "error",
                "message": "mismatched types",
                "code": {"code": "E0308", "explanation": null},
                "spans": [
                    {
                        "file_name": "src/parser/ast.rs",
                        "line_start": 45,
                        "line_end": 45,
                        "column_start": 10,
                        "column_end": 20,
                        "is_primary": true,
                        "label": "expected struct `Node`, found enum `Expr`"
                    },
                    {
                        "file_name": "src/lexer/token.rs",
                        "line_start": 10,
                        "line_end": 10,
                        "column_start": 1,
                        "column_end": 15,
                        "is_primary": false,
                        "label": "type defined here"
                    }
                ]
            }
        }"#;

        let msgs = CargoDiagnosticsParser::parse_json_stream(multi_file_json);
        assert_eq!(msgs.len(), 1);

        let diag = &msgs[0];
        assert_eq!(diag.spans.len(), 2);
        assert!(
            CargoDiagnosticsParser::is_multi_file_span(diag),
            "Diagnostic with spans in ast.rs and token.rs must be detected as multi-file span"
        );

        let primary = CargoDiagnosticsParser::primary_span(diag).expect("Must have primary span");
        assert_eq!(primary.file_name, "src/parser/ast.rs");
        assert!(primary.is_primary);

        // Single file diagnostic should NOT be multi-file span
        let single_file_json = r#"{"reason":"compiler-message","message":{"level":"error","spans":[{"file_name":"src/main.rs","line_start":1,"line_end":1,"column_start":1,"column_end":5,"is_primary":true}]}}"#;
        let single_msgs = CargoDiagnosticsParser::parse_json_stream(single_file_json);
        assert!(!CargoDiagnosticsParser::is_multi_file_span(&single_msgs[0]));
    }

    #[test]
    fn test_harness_cargo_diagnostics_malformed_and_edge_cases() {
        let stream = "
            not valid json at all
            {\"reason\": \"build-plan\"}
            {\"reason\": \"compiler-message\"}
            
        ";
        let msgs = CargoDiagnosticsParser::parse_json_stream(stream);
        // build-plan and empty lines should not crash, compiler-message should be parsed
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].reason, "build-plan");
        assert_eq!(msgs[1].reason, "compiler-message");
        assert!(!CargoDiagnosticsParser::has_errors(&msgs));
    }

    // =========================================================================
    // 4. Deterministic Gate Rules
    // =========================================================================

    #[test]
    fn test_gate_valid_mutation_plan_satisfaction() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 8 cores, 16384 MB RAM
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        let workload = WorkloadBuilder::new("valid-service")
            .kind(WorkloadKind::NodePostgresApi)
            .cpu_cores(4)
            .ram_mb(4096)
            .build();
        let intent = create_intent(vec![workload]);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        assert!(gate_res.is_ok());
        let manifest = gate_res.unwrap();
        assert_eq!(manifest.mappings.len(), 1);
        assert!(DeterministicGate::is_mutation_admissible(&intent, &cluster));
    }

    #[test]
    fn test_gate_rejects_insufficient_cpu() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 8 cores
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload requires 32 cores, only 8 available
        let workload = WorkloadBuilder::new("cpu-hog")
            .cpu_cores(32)
            .ram_mb(2048)
            .build();
        let intent = create_intent(vec![workload]);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        assert!(gate_res.is_err());
        match gate_res.unwrap_err() {
            SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|s| s.contains("requires CPU cores 32")));
            }
            other => panic!("Expected Unsatisfiable error, got {:?}", other),
        }
        assert!(!DeterministicGate::is_mutation_admissible(
            &intent, &cluster
        ));
    }

    #[test]
    fn test_gate_rejects_insufficient_ram() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 16384 MB RAM
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload requires 65536 MB RAM, only 16384 MB available
        let workload = WorkloadBuilder::new("ram-hog")
            .cpu_cores(2)
            .ram_mb(65536)
            .build();
        let intent = create_intent(vec![workload]);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        assert!(gate_res.is_err());
        match gate_res.unwrap_err() {
            SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|s| s.contains("requires RAM 65536MB")));
            }
            other => panic!("Expected Unsatisfiable error, got {:?}", other),
        }
    }

    #[test]
    fn test_gate_rejects_insufficient_gpu_and_vram() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 1 GPU, 8192 MB VRAM
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload requires 4 GPUs and 32768 MB VRAM
        let workload = WorkloadBuilder::new("llm-cluster")
            .kind(WorkloadKind::PythonVllmGpu)
            .gpu_count(4)
            .vram_mb(32768)
            .build();
        let intent = create_intent(vec![workload]);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        assert!(gate_res.is_err());
        match gate_res.unwrap_err() {
            SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core.iter().any(|s| s.contains("requires GPUs 4")));
                assert!(mus_core.iter().any(|s| s.contains("requires VRAM 32768MB")));
            }
            other => panic!("Expected Unsatisfiable error, got {:?}", other),
        }
    }

    #[test]
    fn test_gate_rejects_insufficient_network_bandwidth() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 10 Gbps
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload requires 50 Gbps
        let workload = WorkloadBuilder::new("high-throughput-streamer")
            .add_constraint(Constraint::NetworkGbps(50))
            .build();
        let intent = create_intent(vec![workload]);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        assert!(gate_res.is_err());
        match gate_res.unwrap_err() {
            SolverError::Unsatisfiable { mus_core } => {
                assert!(mus_core
                    .iter()
                    .any(|s| s.contains("requires Network 50Gbps")));
            }
            other => panic!("Expected Unsatisfiable error, got {:?}", other),
        }
    }

    #[test]
    fn test_gate_multi_workload_validation() {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware(); // 8 cores, 16384 MB
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };

        // Workload 1: valid (2 cores, 1024 MB)
        let w1 = WorkloadBuilder::new("web-ui")
            .cpu_cores(2)
            .ram_mb(1024)
            .build();

        // Workload 2: valid (2 cores, 2048 MB)
        let w2 = WorkloadBuilder::new("api-server")
            .cpu_cores(2)
            .ram_mb(2048)
            .build();

        let valid_intent = create_intent(vec![w1.clone(), w2]);
        assert!(DeterministicGate::validate_mutation_plan(&valid_intent, &cluster).is_ok());

        // Workload 3: invalid (99 cores)
        let w3 = WorkloadBuilder::new("invalid-batch").cpu_cores(99).build();

        let mixed_intent = create_intent(vec![w1, w3]);
        let gate_res = DeterministicGate::validate_mutation_plan(&mixed_intent, &cluster);
        assert!(gate_res.is_err());
    }

    // =========================================================================
    // Router & Scheduler Integration Tests
    // =========================================================================

    #[test]
    fn test_universal_model_router_fallback() {
        use router::{ModelProviderKind, UniversalModelRouter};
        let router = UniversalModelRouter::new(
            ModelProviderKind::LocalVllm {
                endpoint: "http://localhost:8000/offline".to_string(),
                model_name: "vllm-model".to_string(),
            },
            Some(ModelProviderKind::Anthropic {
                model_name: "claude-3-5-sonnet".to_string(),
            }),
        );
        let res = router
            .route_completion("Analyze architecture", "System Prompt")
            .unwrap();
        assert!(res.is_fallback);
        assert_eq!(res.provider_used, "Anthropic");
    }

    #[test]
    fn test_gantt_agent_scheduler_dag_execution() {
        use scheduler::{GanttAgentScheduler, GanttTaskNode};
        let mut scheduler = GanttAgentScheduler::new();

        scheduler.add_task(GanttTaskNode {
            id: "task_1".to_string(),
            name: "Parse AST".to_string(),
            subagent_role: "Parser".to_string(),
            dependencies: vec![],
            estimated_duration_ms: 100,
            is_completed: false,
        });

        scheduler.add_task(GanttTaskNode {
            id: "task_2".to_string(),
            name: "Z3 Solve".to_string(),
            subagent_role: "Solver".to_string(),
            dependencies: vec!["task_1".to_string()],
            estimated_duration_ms: 200,
            is_completed: false,
        });

        let tiers = scheduler.compute_parallel_tiers().unwrap();
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0][0].id, "task_1");
        assert_eq!(tiers[1][0].id, "task_2");

        let log = scheduler.execute_dag_schedule().unwrap();
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn test_router_live_gemini_completion_and_token_metering() {
        use router::{ModelProviderKind, UniversalModelRouter};
        let api_key =
            match std::env::var("GEMINI_API_KEY").or_else(|_| std::env::var("GOOGLE_API_KEY")) {
                Ok(k) if !k.is_empty() => k,
                _ => {
                    println!("Skipping live Gemini test: GEMINI_API_KEY not set in environment");
                    return;
                }
            };

        let router = UniversalModelRouter::new(
            ModelProviderKind::Gemini {
                model_name: "gemini-2.5-flash".to_string(),
            },
            None,
        );

        let prompt = "Respond with exactly the single word: ROUTER_HEALTH_OK";
        let system_prompt = "You are a health probe.";
        let res = router
            .route_completion(prompt, system_prompt)
            .expect("Live Gemini routing should succeed");

        assert_eq!(res.provider_used, "Gemini");
        assert_eq!(res.model_used, "gemini-2.5-flash");
        assert!(!res.is_fallback);
        assert!(
            !res.content.is_empty(),
            "Live Gemini response content must not be empty"
        );
        assert!(
            res.tokens_used > 0,
            "Tokens metered must be strictly positive"
        );

        // Verify direct invocation with usage metering
        let direct_res = UniversalModelRouter::call_gemini_api_with_usage(
            &api_key,
            "gemini-2.5-flash",
            "Say ping",
            "System probe",
        );
        assert!(
            direct_res.is_ok(),
            "Direct live Gemini API call should succeed"
        );
        let (content, tokens) = direct_res.unwrap();
        assert!(!content.is_empty(), "Direct content must not be empty");
        assert!(tokens > 0, "Direct token metering must be positive");
    }

    #[test]
    fn test_router_fallback_to_secondary_provider() {
        use router::{ModelProviderKind, UniversalModelRouter};

        // Scenario 1: Primary is unreachable vLLM, fallback is Anthropic Claude
        let router_vllm = UniversalModelRouter::new(
            ModelProviderKind::LocalVllm {
                endpoint: "http://127.0.0.1:8000/offline".to_string(),
                model_name: "vllm-r1".to_string(),
            },
            Some(ModelProviderKind::Anthropic {
                model_name: "claude-3-5-sonnet".to_string(),
            }),
        );
        let res_vllm = router_vllm
            .route_completion("Synthesize microservice plan", "System context")
            .expect("Fallback to Anthropic should succeed");
        assert!(res_vllm.is_fallback);
        assert_eq!(res_vllm.provider_used, "Anthropic");
        assert_eq!(res_vllm.model_used, "claude-3-5-sonnet");
        assert!(res_vllm.tokens_used > 0);

        // Scenario 2: Primary is offline Gemini endpoint, fallback is Anthropic Claude
        let router_gemini = UniversalModelRouter::new(
            ModelProviderKind::Gemini {
                model_name: "offline-gemini-node".to_string(),
            },
            Some(ModelProviderKind::Anthropic {
                model_name: "claude-3-5-sonnet".to_string(),
            }),
        );
        let res_gemini = router_gemini
            .route_completion("Analyze compiler diagnostics", "Diagnostic context")
            .expect("Fallback to Anthropic should succeed");
        assert!(res_gemini.is_fallback);
        assert_eq!(res_gemini.provider_used, "Anthropic");
        assert_eq!(res_gemini.model_used, "claude-3-5-sonnet");

        // Scenario 3: Primary is unreachable Ollama endpoint, fallback is Anthropic Claude
        let router_ollama = UniversalModelRouter::new(
            ModelProviderKind::LocalOllama {
                endpoint: "http://127.0.0.1:11434/unreachable".to_string(),
                model_name: "llama-3".to_string(),
            },
            Some(ModelProviderKind::Anthropic {
                model_name: "claude-3-5-sonnet".to_string(),
            }),
        );
        let res_ollama = router_ollama
            .route_completion("Echo PONG", "Probe")
            .expect("Fallback should succeed");
        assert!(res_ollama.is_fallback);
        assert_eq!(res_ollama.provider_used, "Anthropic");
        assert!(res_ollama.tokens_used > 0);
    }

    #[test]
    fn test_router_unauthenticated_error_when_no_keys_and_local_offline() {
        use router::{ModelProviderKind, UniversalModelRouter};

        std::env::set_var("OLLAMA_ENDPOINT", "http://127.0.0.1:58999/unreachable");
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");

        let router = UniversalModelRouter::new(
            ModelProviderKind::DeepSeek {
                model_name: "deepseek-chat".to_string(),
            },
            None,
        );

        let res = router.route_completion("Synthesize architecture", "System");
        assert!(res.is_err(), "Must return error when unauthenticated and local endpoint unreachable");
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("LLM_PROVIDER_UNAUTHENTICATED: No valid API key (GEMINI_API_KEY, DEEPSEEK_API_KEY, OPENAI_API_KEY) found and local Ollama is not running."),
            "Expected honest error message, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_deepseek_http_api_call_and_metering() {
        use std::io::{Read, Write};
        use router::UniversalModelRouter;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = "{\"choices\":[{\"message\":{\"content\":\"DeepSeek real API generated architecture\"}}],\"usage\":{\"total_tokens\":85}}";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                // Drain socket so client receives response before close
                let mut drain = [0u8; 512];
                let _ = stream.read(&mut drain);
            }
        });

        let url = format!("http://127.0.0.1:{}/chat/completions", port);
        let result = UniversalModelRouter::call_openai_compatible_api(
            &url,
            Some("sk-deepseek-test-key"),
            "deepseek-reasoner",
            "Plan microservice cluster",
            "You are an architect.",
        );

        let _ = handle.join();
        assert!(result.is_ok(), "Real HTTP API call should succeed: {:?}", result.err());
        let (content, tokens) = result.unwrap();
        assert_eq!(content, "DeepSeek real API generated architecture");
        assert_eq!(tokens, 85);
    }

    #[test]
    fn test_router_offline_unreachable_endpoint_error_handling() {
        use router::{ModelProviderKind, UniversalModelRouter};

        // Scenario 1: Local vLLM offline with no fallback configured -> returns Err
        let router_vllm = UniversalModelRouter::new(
            ModelProviderKind::LocalVllm {
                endpoint: "http://127.0.0.1:8000/offline".to_string(),
                model_name: "vllm-r1".to_string(),
            },
            None,
        );
        let res_vllm = router_vllm.route_completion("Generate topology", "IR context");
        assert!(
            res_vllm.is_err(),
            "Offline LocalVllm must return Err when no fallback exists"
        );
        let err_msg = res_vllm.unwrap_err().to_string();
        assert!(err_msg.contains("offline") || err_msg.contains("unreachable"));

        // Scenario 2: Local Ollama unreachable with no fallback configured -> returns Err
        let router_ollama = UniversalModelRouter::new(
            ModelProviderKind::LocalOllama {
                endpoint: "http://127.0.0.1:11434/unreachable".to_string(),
                model_name: "llama3".to_string(),
            },
            None,
        );
        let res_ollama = router_ollama.route_completion("Synthesize plan", "System");
        assert!(
            res_ollama.is_err(),
            "Unreachable LocalOllama must return Err when no fallback exists"
        );
        let err_msg = res_ollama.unwrap_err().to_string();
        assert!(err_msg.contains("offline") || err_msg.contains("unreachable"));

        // Scenario 3: Custom OpenAI compatible unreachable endpoint with no fallback -> returns Err
        let router_custom = UniversalModelRouter::new(
            ModelProviderKind::CustomOpenAiCompatible {
                endpoint: "http://offline-internal.mesh/v1".to_string(),
                model_name: "custom-model".to_string(),
            },
            None,
        );
        let res_custom = router_custom.route_completion("Plan cluster", "System");
        assert!(
            res_custom.is_err(),
            "Unreachable CustomOpenAiCompatible must return Err when no fallback exists"
        );

        // Scenario 4: Gemini with offline model designation and no fallback -> returns Err
        let router_gemini = UniversalModelRouter::new(
            ModelProviderKind::Gemini {
                model_name: "gemini-offline-node".to_string(),
            },
            None,
        );
        let res_gemini = router_gemini.route_completion("Probe status", "System");
        assert!(
            res_gemini.is_err(),
            "Offline Gemini endpoint must return Err when no fallback exists"
        );

        // Scenario 5: Direct Gemini API invocation with invalid API key -> returns Err
        let res_direct_invalid = UniversalModelRouter::call_gemini_api(
            "invalid_api_key_000000000000000000",
            "gemini-2.5-flash",
            "Health probe",
            "",
        );
        assert!(
            res_direct_invalid.is_err(),
            "Call to Gemini with invalid API key must fail"
        );
    }
}
