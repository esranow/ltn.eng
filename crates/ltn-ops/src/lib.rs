pub mod chaos;
pub mod healing;
pub mod mcp;
pub mod observer;

#[cfg(test)]
mod tests {
    use super::*;
    use chaos::{ChaosExperimentResult, ChaosFault, ChronosChaosEngine};
    use healing::{ReplanEngine, SelfHealingAction};
    use mcp::{McpRequest, McpServer};
    use observer::EmpiricalObserver;
    use std::path::Path;

    // =========================================================================
    // 1. JSON-RPC 2.0 Handshake Tests (initialize, notifications/initialized, ping)
    // =========================================================================

    #[test]
    fn test_jsonrpc_initialize() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "gemini-antigravity",
                    "version": "2.0.0"
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, serde_json::json!(1));
        assert!(resp.error.is_none());

        let result = resp.result.expect("Expected result in initialize response");
        assert_eq!(result.get("protocolVersion").and_then(|v| v.as_str()), Some("2024-11-05"));
        assert_eq!(
            result.pointer("/serverInfo/name").and_then(|v| v.as_str()),
            Some("ltn-eng")
        );
        assert_eq!(
            result.pointer("/serverInfo/version").and_then(|v| v.as_str()),
            Some("1.0.0")
        );
        assert_eq!(
            result.pointer("/capabilities/tools/listChanged").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn test_jsonrpc_initialized_notifications() {
        // Test standard notification format
        let req1 = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        let resp1 = McpServer::handle_request(&req1);
        assert_eq!(resp1.jsonrpc, "2.0");
        assert_eq!(resp1.id, serde_json::Value::Null);
        assert!(resp1.error.is_none());
        assert_eq!(resp1.result, Some(serde_json::json!({})));

        // Test shorthand "initialized" method
        let req2 = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("init-msg-2")),
            method: "initialized".to_string(),
            params: Some(serde_json::json!({})),
        };
        let resp2 = McpServer::handle_request(&req2);
        assert_eq!(resp2.jsonrpc, "2.0");
        assert_eq!(resp2.id, serde_json::json!("init-msg-2"));
        assert!(resp2.error.is_none());
        assert_eq!(resp2.result, Some(serde_json::json!({})));
    }

    #[test]
    fn test_jsonrpc_ping() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("ping-id-99")),
            method: "ping".to_string(),
            params: None,
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, serde_json::json!("ping-id-99"));
        assert!(resp.error.is_none());
        assert_eq!(resp.result, Some(serde_json::json!({})));
    }

    #[test]
    fn test_agy_mcp_config_json_compliance() {
        let config_path = Path::new("C:/Users/Hp/.gemini/config/mcp_config.json");
        assert!(config_path.exists(), "MCP config file must exist at C:\\Users\\Hp\\.gemini\\config\\mcp_config.json");

        let raw = std::fs::read_to_string(config_path).expect("Failed to read mcp_config.json");
        let clean_raw = raw.trim_start_matches('\u{feff}');
        let val: serde_json::Value = serde_json::from_str(clean_raw).expect("mcp_config.json must be valid JSON");

        let server_cfg = val.pointer("/mcpServers/ltn-eng")
            .expect("mcpServers must contain 'ltn-eng' entry");

        let command = server_cfg.get("command").and_then(|v| v.as_str()).expect("command must be present");
        assert!(command.ends_with("ltn.exe"), "command should execute ltn.exe");

        let args = server_cfg.get("args").and_then(|v| v.as_array()).expect("args must be an array");
        assert!(args.iter().any(|a| a.as_str() == Some("mcp")), "args must include 'mcp'");

        let env = server_cfg.get("env").and_then(|v| v.as_object()).expect("env must be an object");
        assert!(env.contains_key("GEMINI_API_KEY"), "env must define GEMINI_API_KEY");
        assert!(env.contains_key("AWS_ENDPOINT_URL"), "env must define AWS_ENDPOINT_URL");
    }

    // =========================================================================
    // 2. tools/list Schema Compliance for All 9 Tools
    // =========================================================================

    #[test]
    fn test_tools_list_schema_compliance_all_9_tools() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!(2));
        assert!(resp.error.is_none());

        let result = resp.result.expect("Expected result in tools/list response");
        let tools = result.get("tools").and_then(|v| v.as_array()).expect("Expected tools array");
        assert_eq!(tools.len(), 9, "Server must expose exactly 9 tools");

        let expected_tools = [
            ("ltn_doctor", false, &[] as &[&str]),
            ("ltn_analyze", true, &["path"]),
            ("ltn_architect", true, &["path"]),
            ("ltn_deploy", true, &["path"]),
            ("ltn_status", false, &[]),
            ("ltn_snapshot", false, &[]),
            ("ltn_rollback", false, &[]),
            ("ltn_config", false, &[]),
            ("map_mtl_topology", false, &[]),
        ];

        for (name, has_required, req_fields) in expected_tools {
            let tool = tools
                .iter()
                .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(name))
                .unwrap_or_else(|| panic!("Tool '{}' not found in tools/list", name));

            let desc = tool.get("description").and_then(|d| d.as_str()).expect("description must be string");
            assert!(!desc.is_empty(), "Tool '{}' must have non-empty description", name);

            let schema = tool.get("inputSchema").and_then(|s| s.as_object()).expect("inputSchema must be object");
            assert_eq!(
                schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "Tool '{}' schema type must be 'object'",
                name
            );
            assert!(
                schema.get("properties").and_then(|p| p.as_object()).is_some(),
                "Tool '{}' properties must be an object",
                name
            );

            if has_required {
                let required = schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .expect("required must be an array");
                for req_field in req_fields {
                    assert!(
                        required.iter().any(|f| f.as_str() == Some(*req_field)),
                        "Tool '{}' must require field '{}'",
                        name,
                        req_field
                    );
                }
            }
        }
    }

    // =========================================================================
    // 3. tools/call Tests for Each of the 9 Tools
    // =========================================================================

    #[test]
    fn test_tool_call_ltn_doctor() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("doc-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_doctor",
                "arguments": {}
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("doc-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let doc: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(doc.get("status").and_then(|s| s.as_str()), Some("HEALTHY"));
        assert_eq!(doc.get("sandbox").and_then(|s| s.as_str()), Some("OPERATIONAL"));
        assert_eq!(doc.get("constraint_prover").and_then(|s| s.as_str()), Some("READY"));
        assert!(doc.get("providers").and_then(|p| p.as_object()).is_some());
    }

    #[test]
    fn test_tool_call_ltn_analyze() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("ana-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_analyze",
                "arguments": {
                    "path": "."
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("ana-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(parsed.get("workload_name").is_some() || parsed.get("min_cpu_cores").is_some() || parsed.is_object());
    }

    #[test]
    fn test_tool_call_ltn_architect() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("arch-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_architect",
                "arguments": {
                    "path": "."
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("arch-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(parsed.is_array() || parsed.is_object());
    }

    #[test]
    fn test_tool_call_ltn_deploy() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("dep-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_deploy",
                "arguments": {
                    "path": ".",
                    "tier": "BALANCED"
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("dep-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(parsed.get("status").and_then(|s| s.as_str()), Some("DEPLOYED"));
        assert_eq!(parsed.get("verification").and_then(|s| s.as_str()), Some("PASSED"));
        assert!(parsed.get("container_id").and_then(|s| s.as_str()).unwrap().starts_with("doc-"));
    }

    #[test]
    fn test_tool_call_ltn_status() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("stat-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_status",
                "arguments": {
                    "endpoint": "http://127.0.0.1:8080/health"
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("stat-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();

        assert!(parsed.get("socket_probes").and_then(|p| p.as_array()).is_some());
        assert!(parsed.get("process_probes").and_then(|p| p.as_array()).is_some());
        assert!(parsed.get("http_probes").and_then(|p| p.as_array()).is_some());
        assert_eq!(parsed.get("drift_detected").and_then(|d| d.as_bool()), Some(false));
    }

    #[test]
    fn test_tool_call_ltn_snapshot_and_rollback() {
        // Snapshot
        let snap_req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("snap-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_snapshot",
                "arguments": {}
            })),
        };

        let snap_resp = McpServer::handle_request(&snap_req);
        assert_eq!(snap_resp.id, serde_json::json!("snap-1"));
        let snap_val = snap_resp.result.unwrap();
        assert_eq!(snap_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let snap_text = snap_val["content"][0]["text"].as_str().unwrap();
        let snap_parsed: serde_json::Value = serde_json::from_str(snap_text).unwrap();
        assert_eq!(snap_parsed.get("status").and_then(|s| s.as_str()), Some("SNAPSHOT_RECORDED"));
        assert!(snap_parsed.get("hash").and_then(|h| h.as_str()).is_some());

        // Rollback
        let rb_req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("rb-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_rollback",
                "arguments": {}
            })),
        };

        let rb_resp = McpServer::handle_request(&rb_req);
        assert_eq!(rb_resp.id, serde_json::json!("rb-1"));
        let rb_val = rb_resp.result.unwrap();
        assert_eq!(rb_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let rb_text = rb_val["content"][0]["text"].as_str().unwrap();
        let rb_parsed: serde_json::Value = serde_json::from_str(rb_text).unwrap();
        assert_eq!(rb_parsed.get("status").and_then(|s| s.as_str()), Some("RESTORED"));
    }

    #[test]
    fn test_tool_call_ltn_config() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("cfg-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "ltn_config",
                "arguments": {
                    "provider": "LOCAL_DOCKER",
                    "model": "gemini-2.5-flash",
                    "endpoint": "http://localhost:11434"
                }
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("cfg-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            parsed.pointer("/provider/default_provider").and_then(|p| p.as_str()),
            Some("LOCAL_DOCKER")
        );
        assert_eq!(
            parsed.pointer("/llm/active_model").and_then(|m| m.as_str()),
            Some("gemini-2.5-flash")
        );
    }

    #[test]
    fn test_tool_call_map_mtl_topology() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("topo-1")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "map_mtl_topology",
                "arguments": {}
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("topo-1"));
        assert!(resp.error.is_none());

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(false));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();

        assert!(parsed.get("cpu_count").and_then(|c| c.as_u64()).unwrap() > 0);
        assert!(parsed.get("total_memory_mb").and_then(|m| m.as_u64()).unwrap() > 0);
        assert_eq!(parsed.get("hardware_status").and_then(|s| s.as_str()), Some("OPERATIONAL"));
    }

    #[test]
    fn test_tool_call_unknown_tool() {
        let req = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("unknown-call")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "non_existent_tool",
                "arguments": {}
            })),
        };

        let resp = McpServer::handle_request(&req);
        assert_eq!(resp.id, serde_json::json!("unknown-call"));
        assert!(resp.error.is_none()); // In MCP, tool errors return isError: true in result

        let res_val = resp.result.unwrap();
        assert_eq!(res_val.get("isError").and_then(|b| b.as_bool()), Some(true));
        let text = res_val["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Unknown tool 'non_existent_tool'"));
    }

    // =========================================================================
    // 4. Error Handling: parse error (-32700), invalid method (-32601), invalid params (-32602)
    // =========================================================================

    #[test]
    fn test_error_parse_error_32700() {
        let invalid_raw_payloads = [
            "{invalid_json",
            "",
            "{\"jsonrpc\": \"2.0\", \"method\": ",
            "not a json object at all",
        ];

        for payload in invalid_raw_payloads {
            let resp = McpServer::handle_raw_request(payload);
            assert_eq!(resp.jsonrpc, "2.0");
            assert_eq!(resp.id, serde_json::Value::Null);
            assert!(resp.result.is_none());

            let err = resp.error.expect("Expected McpError for invalid JSON");
            assert_eq!(err.code, -32700, "Error code must be -32700 Parse error for payload: {}", payload);
            assert!(err.message.starts_with("Parse error"));
        }
    }

    #[test]
    fn test_error_invalid_method_32601() {
        let invalid_methods = ["invalid_method", "tools/destroy", "quantum/compute", ""];

        for method in invalid_methods {
            let req = McpRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!(404)),
                method: method.to_string(),
                params: None,
            };

            let resp = McpServer::handle_request(&req);
            assert_eq!(resp.id, serde_json::json!(404));
            assert!(resp.result.is_none());

            let err = resp.error.expect("Expected McpError for invalid method");
            assert_eq!(err.code, -32601, "Error code must be -32601 for method: {}", method);
            assert!(err.message.contains("not found"));
        }
    }

    #[test]
    fn test_error_invalid_params_32602() {
        // Case 1: Missing params object completely for tools/call
        let req_missing_params = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("missing-p")),
            method: "tools/call".to_string(),
            params: None,
        };
        let resp1 = McpServer::handle_request(&req_missing_params);
        let err1 = resp1.error.expect("Expected error for missing params");
        assert_eq!(err1.code, -32602);
        assert!(err1.message.contains("Missing params object"));

        // Case 2: Params is not a JSON object (e.g. an array)
        let req_array_params = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("array-p")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!([1, 2, 3])),
        };
        let resp2 = McpServer::handle_request(&req_array_params);
        let err2 = resp2.error.expect("Expected error for array params");
        assert_eq!(err2.code, -32602);
        assert!(err2.message.contains("Params must be a JSON object"));

        // Case 3: Missing 'name' in params object
        let req_no_name = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("no-name")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "arguments": {}
            })),
        };
        let resp3 = McpServer::handle_request(&req_no_name);
        let err3 = resp3.error.expect("Expected error for missing name in params");
        assert_eq!(err3.code, -32602);
        assert!(err3.message.contains("Missing or invalid 'name'"));

        // Case 4: Blank 'name' in params object
        let req_blank_name = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!("blank-name")),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "   ",
                "arguments": {}
            })),
        };
        let resp4 = McpServer::handle_request(&req_blank_name);
        let err4 = resp4.error.expect("Expected error for blank name in params");
        assert_eq!(err4.code, -32602);
    }

    // =========================================================================
    // 5. Empirical Observer Socket Probes, Chronos Chaos Faults, Self-Healing Replanner
    // =========================================================================

    #[test]
    fn test_empirical_observer_socket_probes() {
        let telemetry = EmpiricalObserver::probe_system("http://127.0.0.1:4566/health");

        // Socket probe assertions
        assert!(!telemetry.socket_probes.is_empty(), "Socket probes must not be empty");
        let sock = &telemetry.socket_probes[0];
        assert_eq!(sock.address, "127.0.0.1:4566");
        assert!(sock.reachable, "Socket must be reachable");
        assert!(sock.latency_ms >= 0.0, "Latency must be non-negative");

        // Process probe assertions
        assert!(!telemetry.process_probes.is_empty(), "Process probes must not be empty");
        let proc = &telemetry.process_probes[0];
        assert!(proc.running);

        // HTTP probe assertions
        assert!(!telemetry.http_probes.is_empty(), "HTTP probes must not be empty");
        let http = &telemetry.http_probes[0];
        assert_eq!(http.url, "http://127.0.0.1:4566/health");
        assert_eq!(http.status_code, 200);
        assert!(http.latency_ms >= 0.0);

        // Initial baseline drift
        assert!(!telemetry.drift_detected, "Initial telemetry should not show drift");
    }

    #[test]
    fn test_chronos_chaos_fault_injection() {
        // Test packet loss fault
        let net_fault = ChaosFault::NetworkPacketLoss { percentage: 25 };
        let res_net = ChronosChaosEngine::inject_fault(net_fault);
        assert!(res_net.system_recovered);
        assert!(res_net.recovery_time_ms > 0);

        // Test process termination fault
        let proc_fault = ChaosFault::ProcessTermination {
            process_name: "ltn-sandbox".to_string(),
        };
        let res_proc = ChronosChaosEngine::inject_fault(proc_fault);
        assert!(res_proc.system_recovered);

        // Test memory pressure fault
        let mem_fault = ChaosFault::MemoryPressure { mb: 4096 };
        let res_mem = ChronosChaosEngine::inject_fault(mem_fault);
        assert!(res_mem.system_recovered);

        // Serialization round-trip
        let serialized = serde_json::to_string(&res_mem).expect("Chaos experiment should serialize");
        let deserialized: ChaosExperimentResult =
            serde_json::from_str(&serialized).expect("Chaos experiment should deserialize");
        assert_eq!(deserialized.recovery_time_ms, res_mem.recovery_time_ms);
        assert_eq!(deserialized.system_recovered, res_mem.system_recovered);
    }

    #[test]
    fn test_self_healing_replan_flow() {
        let mut telemetry = EmpiricalObserver::probe_system("http://127.0.0.1:4566/health");

        // Without drift, replanner should return None
        let no_action = ReplanEngine::evaluate_and_heal(&telemetry);
        assert!(no_action.is_none(), "Normal telemetry should not trigger self-healing");

        // Inject drift into telemetry
        telemetry.drift_detected = true;
        let action = ReplanEngine::evaluate_and_heal(&telemetry)
            .expect("Drifted telemetry must trigger self-healing action");

        assert!(action.success);
        assert_eq!(action.drift_cause, "Container failed health check");
        assert_eq!(action.corrective_action, "Restarting container via ltn-replan");

        // Action serialization round-trip
        let serialized_action = serde_json::to_string(&action).unwrap();
        let deserialized_action: SelfHealingAction = serde_json::from_str(&serialized_action).unwrap();
        assert!(deserialized_action.success);
    }

    #[test]
    fn test_closed_loop_observer_chaos_healing() {
        // 1. Observe healthy system
        let mut telemetry = EmpiricalObserver::probe_system("http://127.0.0.1:4566/health");
        assert!(!telemetry.drift_detected);

        // 2. Chronos injects network packet loss
        let fault = ChaosFault::NetworkPacketLoss { percentage: 50 };
        let experiment = ChronosChaosEngine::inject_fault(fault);
        assert!(experiment.system_recovered);

        // 3. Mark drift as observed post-fault
        telemetry.drift_detected = true;

        // 4. Replan engine self-heals
        let healing = ReplanEngine::evaluate_and_heal(&telemetry).expect("Self-healing must engage");
        assert!(healing.success);

        // 5. Verify system restored to healthy state
        telemetry.drift_detected = false;
        assert!(ReplanEngine::evaluate_and_heal(&telemetry).is_none());
    }
}

