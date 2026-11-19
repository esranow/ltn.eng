use super::permissions::{AgentCapability, CapabilityContext};
use super::GuardrailEnforcer;
use crate::ir::{Architecture, ProviderTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyViolationType {
    PlaintextSecretDetected,
    MissingSnapshot,
    UnauthorizedMutation,
    BudgetExceeded,
    UnsupportedProviderResource,
    SinglePointOfFailure,
    CanaryBypassed,
    SloViolation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyViolation {
    pub rule: PolicyViolationType,
    pub description: String,
    pub offending_resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyEvaluationResult {
    pub approved: bool,
    pub violations: Vec<PolicyViolation>,
    pub evaluated_rules_count: usize,
    pub timestamp: String,
}

pub struct PolicyEngine;

impl PolicyEngine {
    /// Evaluates an Architecture IR and mutation context against deterministic operational and security policies
    pub fn evaluate_architecture_and_context(
        arch: &Architecture,
        context: &CapabilityContext,
        has_prior_snapshot: bool,
        is_production: bool,
    ) -> PolicyEvaluationResult {
        let mut violations = Vec::new();
        let mut rules_count = 0;

        // Rule 1: No Plaintext Secrets in Architecture Specification
        rules_count += 1;
        for w in &arch.workloads {
            for secret in &w.secrets {
                if secret.vault_key.trim().is_empty() {
                    violations.push(PolicyViolation {
                        rule: PolicyViolationType::PlaintextSecretDetected,
                        description: format!(
                            "Secret reference '{}' has empty vault key reference.",
                            secret.env_var_name
                        ),
                        offending_resource: w.name.clone(),
                    });
                }
            }
            // Scan image and entrypoint for hardcoded tokens
            let full_text = format!("{} {:?}", w.container_image, w.entrypoint);
            if let Some(leak) = GuardrailEnforcer::scan_for_plaintext_secrets(&full_text, 0) {
                violations.push(PolicyViolation {
                    rule: PolicyViolationType::PlaintextSecretDetected,
                    description: leak.reason,
                    offending_resource: w.name.clone(),
                });
            }
        }

        // Rule 2: Mandatory Prior Snapshot for any Mutation
        rules_count += 1;
        if !has_prior_snapshot {
            violations.push(PolicyViolation {
                rule: PolicyViolationType::MissingSnapshot,
                description: "State snapshot required before applying architecture mutation."
                    .to_string(),
                offending_resource: arch.metadata.name.clone(),
            });
        }

        // Rule 3: Capability and Authorization Check
        rules_count += 1;
        let required_cap = if is_production {
            AgentCapability::DeployProduction
        } else {
            AgentCapability::DeployStaging
        };
        if let Err(err_msg) = context.require_capability(required_cap) {
            violations.push(PolicyViolation {
                rule: PolicyViolationType::UnauthorizedMutation,
                description: err_msg,
                offending_resource: arch.metadata.name.clone(),
            });
        }

        // Rule 4: Budget Compliance
        rules_count += 1;
        // Basic resource pricing sanity check against max monthly budget
        let total_cores: u32 = arch
            .workloads
            .iter()
            .map(|w| w.compute.cpu_cores * w.replicas.desired_replicas)
            .sum();
        let total_ram_gb: u32 = arch
            .workloads
            .iter()
            .map(|w| (w.compute.ram_mb * w.replicas.desired_replicas) / 1024)
            .sum();
        let total_gpus: u32 = arch
            .workloads
            .iter()
            .map(|w| w.compute.gpu_count * w.replicas.desired_replicas)
            .sum();
        let estimated_monthly =
            (total_cores as f64 * 25.0) + (total_ram_gb as f64 * 4.0) + (total_gpus as f64 * 350.0);
        if estimated_monthly > arch.cost_constraints.max_monthly_budget_usd {
            violations.push(PolicyViolation {
                rule: PolicyViolationType::BudgetExceeded,
                description: format!(
                    "Estimated architecture cost ${:.2}/mo exceeds maximum declared budget ${:.2}/mo.",
                    estimated_monthly, arch.cost_constraints.max_monthly_budget_usd
                ),
                offending_resource: "CostConstraints".to_string(),
            });
        }

        // Rule 5: Supported Provider Resource
        rules_count += 1;
        if arch.provider_target == ProviderTarget::LocalDocker && total_gpus > 0 {
            // LocalDocker with GPU requires explicit check
        }
        if arch.provider_target == ProviderTarget::BareMetal && arch.availability.multi_az_required
        {
            violations.push(PolicyViolation {
                rule: PolicyViolationType::UnsupportedProviderResource,
                description: "Multi-AZ availability requirement is incompatible with single-site BareMetal provider.".to_string(),
                offending_resource: "AvailabilityRequirements".to_string(),
            });
        }

        // Rule 6: High Availability Quorum / No Single Point of Failure
        rules_count += 1;
        if arch.availability.multi_az_required {
            for w in &arch.workloads {
                if w.replicas.desired_replicas < 2 {
                    violations.push(PolicyViolation {
                        rule: PolicyViolationType::SinglePointOfFailure,
                        description: format!(
                            "Workload '{}' has only {} replica, but High Availability requires at least 2 replicas.",
                            w.name, w.replicas.desired_replicas
                        ),
                        offending_resource: w.name.clone(),
                    });
                }
            }
        }
        for w in &arch.workloads {
            if w.kind == crate::ir::WorkloadKind::StatefulStore && w.replicas.desired_replicas < 3 {
                violations.push(PolicyViolation {
                    rule: PolicyViolationType::SinglePointOfFailure,
                    description: format!(
                        "Distributed stateful store '{}' has desired_replicas={}, which violates consensus quorum requirements (min 3 replicas).",
                        w.name, w.replicas.desired_replicas
                    ),
                    offending_resource: w.name.clone(),
                });
            }
        }

        // Rule 7: Mandatory Canary for Production
        rules_count += 1;
        if is_production && arch.availability.sla_target_pct >= 99.9 {
            // Production deployment with >= 99.9% SLA requires canary verification
            violations.push(PolicyViolation {
                rule: PolicyViolationType::CanaryBypassed,
                description: format!(
                    "Production deployment with declared SLA {:.2}% requires mandatory canary rollout, but canary was bypassed.",
                    arch.availability.sla_target_pct
                ),
                offending_resource: "DeploymentPolicy".to_string(),
            });
        }

        // Rule 8: SLO Constraints
        rules_count += 1;
        if arch.availability.max_p99_latency_ms <= 0.0
            || arch.availability.max_p99_latency_ms > 2000.0
            || (arch.failure_domains.len() > 1 && arch.availability.max_p99_latency_ms < 5.0)
        {
            violations.push(PolicyViolation {
                rule: PolicyViolationType::SloViolation,
                description: format!(
                    "Unrealistic or unsafe SLO latency target declared: {}ms (multi-domain requires >= 5ms)",
                    arch.availability.max_p99_latency_ms
                ),
                offending_resource: "AvailabilityRequirements".to_string(),
            });
        }

        let approved = violations.is_empty();

        PolicyEvaluationResult {
            approved,
            violations,
            evaluated_rules_count: rules_count,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;
    use uuid::Uuid;

    #[test]
    fn test_policy_engine_passes_clean_architecture() {
        let mut arch = Architecture::new("clean-service", ProviderTarget::LocalDocker);
        arch.workloads.push(WorkloadDefinition {
            id: Uuid::new_v4(),
            name: "api".to_string(),
            kind: WorkloadKind::NodePostgresApi,
            container_image: "node:18".to_string(),
            entrypoint: None,
            ports: vec![],
            secrets: vec![SecretReference {
                env_var_name: "DB_PASS".to_string(),
                vault_key: "DB_PASSWORD_KEY".to_string(),
                optional: false,
            }],
            compute: ComputeRequirements::default(),
            storage: StorageRequirements::default(),
            network: NetworkRequirements::default(),
            replicas: ReplicaPolicy::default(),
            placement: PlacementConstraints::default(),
        });

        let context = CapabilityContext::default();
        let res = PolicyEngine::evaluate_architecture_and_context(&arch, &context, true, false);
        assert!(
            res.approved,
            "Clean architecture with snapshot must be approved: {:?}",
            res.violations
        );
    }

    #[test]
    fn test_policy_engine_blocks_missing_snapshot() {
        let arch = Architecture::new("clean-service", ProviderTarget::LocalDocker);
        let context = CapabilityContext::default();
        let res = PolicyEngine::evaluate_architecture_and_context(&arch, &context, false, false);
        assert!(!res.approved);
        assert!(res
            .violations
            .iter()
            .any(|v| v.rule == PolicyViolationType::MissingSnapshot));
    }

    #[test]
    fn test_policy_engine_blocks_unauthorized_production() {
        let arch = Architecture::new("clean-service", ProviderTarget::LocalDocker);
        let context = CapabilityContext::default(); // default only has staging capability
        let res = PolicyEngine::evaluate_architecture_and_context(&arch, &context, true, true);
        assert!(!res.approved);
        assert!(res
            .violations
            .iter()
            .any(|v| v.rule == PolicyViolationType::UnauthorizedMutation));
    }
}
