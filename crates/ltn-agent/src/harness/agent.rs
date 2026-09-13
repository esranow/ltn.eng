use super::super::gate::DeterministicGate;
use super::super::registry::{AgentToolKind, ToolCall, ToolRegistry, ToolResult};
use ltn_core::ir::{create_intent, ClusterState, Intent, WorkloadBuilder};
use ltn_core::state_machine::StateMachine;
use ltn_core::topology::TopologyDiscoveryAdapter;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HarnessPhase {
    Read,
    Plan,
    Validate,
    Mutate,
    Verify,
    Rollback,
    Completed,
}

pub struct AgentHarness {
    pub state_machine: StateMachine,
    pub current_phase: HarnessPhase,
    pub phase_history: Vec<HarnessPhase>,
}

impl Default for AgentHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentHarness {
    pub fn new() -> Self {
        Self {
            state_machine: StateMachine::new(),
            current_phase: HarnessPhase::Read,
            phase_history: vec![HarnessPhase::Read],
        }
    }

    pub fn run_lifecycle(&mut self, target_project: &str) -> Vec<ToolResult> {
        let adapter = TopologyDiscoveryAdapter::new();
        let hardware = adapter.discover_local_hardware();
        let cluster = ClusterState {
            resources: vec![hardware],
            network_latencies: std::collections::HashMap::new(),
        };
        let workload = WorkloadBuilder::new(target_project)
            .cpu_cores(2)
            .ram_mb(2048)
            .build();
        let intent = create_intent(vec![workload]);

        self.run_lifecycle_with_intent(target_project, intent, cluster)
    }

    pub fn run_lifecycle_with_intent(
        &mut self,
        target_project: &str,
        intent: Intent,
        cluster: ClusterState,
    ) -> Vec<ToolResult> {
        let mut results = Vec::new();

        // 1. READ
        self.current_phase = HarnessPhase::Read;
        let read_call = ToolCall {
            tool: AgentToolKind::Read,
            target: target_project.to_string(),
            payload: String::new(),
        };
        results.push(ToolRegistry::execute_tool(&read_call));

        // 2. PLAN
        self.current_phase = HarnessPhase::Plan;
        self.phase_history.push(HarnessPhase::Plan);
        let plan_call = ToolCall {
            tool: AgentToolKind::Plan,
            target: target_project.to_string(),
            payload: String::new(),
        };
        results.push(ToolRegistry::execute_tool(&plan_call));

        // 3. VALIDATE (Deterministic Gate Z3 Check)
        self.current_phase = HarnessPhase::Validate;
        self.phase_history.push(HarnessPhase::Validate);

        let gate_res = DeterministicGate::validate_mutation_plan(&intent, &cluster);
        if gate_res.is_err() {
            let rollback_call = ToolCall {
                tool: AgentToolKind::Rollback,
                target: target_project.to_string(),
                payload: "Z3 validation failed".to_string(),
            };
            results.push(ToolRegistry::execute_tool(&rollback_call));
            self.current_phase = HarnessPhase::Rollback;
            self.phase_history.push(HarnessPhase::Rollback);
            return results;
        }

        let val_call = ToolCall {
            tool: AgentToolKind::Validate,
            target: target_project.to_string(),
            payload: String::new(),
        };
        results.push(ToolRegistry::execute_tool(&val_call));

        // 4. MUTATE
        self.current_phase = HarnessPhase::Mutate;
        self.phase_history.push(HarnessPhase::Mutate);
        self.state_machine.set_key("status", "mutating");
        self.state_machine.commit();
        let mutate_call = ToolCall {
            tool: AgentToolKind::Mutate,
            target: target_project.to_string(),
            payload: "Applied architectural manifest update".to_string(),
        };
        results.push(ToolRegistry::execute_tool(&mutate_call));

        // 5. VERIFY
        self.current_phase = HarnessPhase::Verify;
        self.phase_history.push(HarnessPhase::Verify);
        let verify_call = ToolCall {
            tool: AgentToolKind::Verify,
            target: target_project.to_string(),
            payload: String::new(),
        };
        results.push(ToolRegistry::execute_tool(&verify_call));

        self.current_phase = HarnessPhase::Completed;
        self.phase_history.push(HarnessPhase::Completed);

        results
    }
}
