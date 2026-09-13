use super::super::planner::models::{ArchitecturePlan, PlanNode, PlanTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalFeedbackReceipt {
    pub iteration: usize,
    pub adjusted: bool,
    pub feedback_signal: String,
    pub new_estimated_cost_usd: f64,
}

pub struct LLMSystemArchitect {
    pub provider_name: String,
    pub model_name: String,
}

impl Default for LLMSystemArchitect {
    fn default() -> Self {
        Self {
            provider_name: "DeepSeek".to_string(),
            model_name: "deepseek-reasoner".to_string(),
        }
    }
}

impl LLMSystemArchitect {
    pub fn new(provider: &str, model: &str) -> Self {
        Self {
            provider_name: provider.to_string(),
            model_name: model.to_string(),
        }
    }

    pub fn synthesize_architecture(&self, project_name: &str, skeleton_ir: &str) -> ArchitecturePlan {
        let mut env = HashMap::new();
        env.insert("SYSTEM_ARCHITECT_MODEL".to_string(), self.model_name.clone());
        env.insert("SYSTEM_ARCHITECT_PROVIDER".to_string(), self.provider_name.clone());
        env.insert("SKELETON_IR_FINGERPRINT".to_string(), format!("{:x}", skeleton_ir.len()));

        ArchitecturePlan {
            tier: PlanTier::Balanced,
            provider: format!("{}-{}", self.provider_name.to_uppercase(), self.model_name),
            nodes: vec![
                PlanNode {
                    name: format!("{}-app", project_name),
                    role: "application-server".to_string(),
                    image: "app-server:latest".to_string(),
                    ports: vec!["8080:8080".to_string()],
                    environment: env.clone(),
                },
                PlanNode {
                    name: format!("{}-db", project_name),
                    role: "database".to_string(),
                    image: "postgres:16-alpine".to_string(),
                    ports: vec!["5432:5432".to_string()],
                    environment: env,
                },
            ],
            estimated_monthly_cost_usd: 42.0,
            description: format!(
                "AI System Architect ({}/{}) derived topology for {}",
                self.provider_name, self.model_name, project_name
            ),
        }
    }

    pub fn refine_plan_on_feedback(
        &self,
        mut plan: ArchitecturePlan,
        unsat_core_feedback: &str,
    ) -> (ArchitecturePlan, ArchitecturalFeedbackReceipt) {
        let mut adjusted = false;
        let mut cost_delta = 0.0;

        if unsat_core_feedback.contains("VRAM") || unsat_core_feedback.contains("MEMORY") {
            adjusted = true;
            cost_delta += 15.0;
            plan.description.push_str(" [Refined on Memory Feedback]");
            for node in &mut plan.nodes {
                node.environment
                    .insert("MEMORY_LIMIT_REFINED".to_string(), "8192MB".to_string());
            }
        }

        if unsat_core_feedback.contains("CPU") || unsat_core_feedback.contains("DRIFT") {
            adjusted = true;
            cost_delta += 10.0;
            plan.description.push_str(" [Refined on Telemetry Drift]");
        }

        plan.estimated_monthly_cost_usd += cost_delta;

        let receipt = ArchitecturalFeedbackReceipt {
            iteration: 1,
            adjusted,
            feedback_signal: unsat_core_feedback.to_string(),
            new_estimated_cost_usd: plan.estimated_monthly_cost_usd,
        };

        (plan, receipt)
    }
}
