# LTN.ENG — Crate API Reference

## 1. `ltn-core` Crate API

### `ltn_core::ir`
- `WorkloadBuilder::new(name: impl Into<String>) -> WorkloadBuilder`
  - `.cpu_cores(cores: u32) -> Self`
  - `.ram_mb(mb: u32) -> Self`
  - `.vram_mb(mb: u32) -> Self`
  - `.gpu_count(gpus: u32) -> Self`
  - `.build() -> Workload`
- `create_intent(workloads: Vec<Workload>) -> Intent`

### `ltn_core::solver`
- `SmtSolver::solve(intent: &Intent, cluster: &ClusterState) -> Result<ExecManifest, SolverError>`
- `DependencyGraph<T>::new() -> DependencyGraph<T>`
  - `.add_edge(from: T, to: T)`
  - `.topological_sort() -> Result<Vec<T>, SolverError>`

### `ltn_core::crypto`
- `StateHasher::hash_bytes(data: &[u8]) -> String`
- `StateHasher::hash_json<T: Serialize>(val: &T) -> String`

### `ltn_core::state_machine`
- `StateMachine::new() -> StateMachine`
  - `.set_key(key, val)`
  - `.commit() -> StateSnapshot`
  - `.rollback() -> Option<StateSnapshot>`

### `ltn_core::config`
- `LtnSettings::load_from_file(path: &Path) -> Result<Self, String>`
- `LtnSettings::save_to_file(&self, path: &Path) -> Result<(), String>`

---

## 2. `ltn-architect` Crate API

### `ltn_architect::parsers`
- `ManifestParser::discover_in_dir(path: &Path) -> ManifestDiscovery`

### `ltn_architect::classifier`
- `WorkloadClassifier::classify(manifests: &ManifestDiscovery, dir_name: &str) -> ProjectClassification`

### `ltn_architect::planner`
- `ArchitecturePlanner::generate_plans(classification: &ProjectClassification) -> Vec<ArchitecturePlan>`

### `ltn_architect::artifacts`
- `MissingArtifactGenerator::generate_missing_artifacts(classification: &ProjectClassification, has_dockerfile: bool) -> Vec<GeneratedArtifact>`

---

## 3. `ltn-agent` Crate API

### `ltn_agent::harness`
- `AgentHarness::new() -> AgentHarness`
  - `.run_lifecycle(target_project: &str) -> Vec<ToolResult>`

### `ltn_agent::gate`
- `DeterministicGate::validate_mutation_plan(intent: &Intent, cluster: &ClusterState) -> Result<ExecManifest, SolverError>`

### `ltn_agent::registry`
- `ToolRegistry::execute_tool(call: &ToolCall) -> ToolResult`

---

## 4. `ltn-providers` Crate API

### `ltn_providers::auth`
- `AuthChecker::check_provider_auth(provider_name: &str) -> AuthStatus`

### `ltn_providers::actuators`
- `DockerActuator::deploy_container(image: &str, name: &str, port: &str) -> DockerActuatorResult`
- `AwsActuator::deploy_ecs_task(task_name: &str) -> Result<AwsActuatorResult, String>`
- `GcpActuator::deploy_gke_workload(workload_name: &str) -> Result<GcpActuatorResult, String>`
- `AzureActuator::deploy_aks_cluster(cluster_name: &str) -> Result<AzureActuatorResult, String>`
- `LocalOsActuator::spawn_process(command: &str) -> LocalOsActuatorResult`
- `BareMetalActuator::provision_node(node_ip: &str) -> BareMetalActuatorResult`

---

## 5. `ltn-ops` Crate API

### `ltn_ops::observer`
- `EmpiricalObserver::probe_system(target_url: &str) -> ObservationTelemetry`

### `ltn_ops::healing`
- `ReplanEngine::evaluate_and_heal(telemetry: &ObservationTelemetry) -> Option<SelfHealingAction>`

### `ltn_ops::chaos`
- `ChronosChaosEngine::inject_fault(fault: ChaosFault) -> ChaosExperimentResult`

### `ltn_ops::mcp`
- `McpServer::handle_request(req: &McpRequest) -> McpResponse`
