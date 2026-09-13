use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GanttTaskNode {
    pub id: String,
    pub name: String,
    pub subagent_role: String,
    pub dependencies: Vec<String>,
    pub estimated_duration_ms: u64,
    pub is_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GanttAgentScheduler {
    pub tasks: Vec<GanttTaskNode>,
}

impl GanttAgentScheduler {
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn add_task(&mut self, task: GanttTaskNode) {
        self.tasks.push(task);
    }

    pub fn compute_parallel_tiers(&self) -> anyhow::Result<Vec<Vec<GanttTaskNode>>> {
        let mut completed = HashSet::new();
        let mut remaining: HashMap<String, GanttTaskNode> = self
            .tasks
            .iter()
            .cloned()
            .map(|t| (t.id.clone(), t))
            .collect();

        let mut tiers = Vec::new();

        while !remaining.is_empty() {
            let mut current_tier = Vec::new();

            for (id, task) in &remaining {
                if task.dependencies.iter().all(|dep| completed.contains(dep)) {
                    current_tier.push(id.clone());
                }
            }

            if current_tier.is_empty() {
                anyhow::bail!("Cyclic dependency detected in Gantt task graph");
            }

            let tier_nodes: Vec<GanttTaskNode> = current_tier
                .iter()
                .filter_map(|id| remaining.remove(id))
                .collect();

            for node in &tier_nodes {
                completed.insert(node.id.clone());
            }

            tiers.push(tier_nodes);
        }

        Ok(tiers)
    }

    pub fn execute_dag_schedule(&mut self) -> anyhow::Result<Vec<String>> {
        let tiers = self.compute_parallel_tiers()?;
        let mut execution_log = Vec::new();

        for (tier_idx, tier) in tiers.into_iter().enumerate() {
            let task_names: Vec<String> = tier.iter().map(|t| t.name.clone()).collect();
            execution_log.push(format!(
                "Gantt Tier {}: Spawning {} parallel subagents [{}]",
                tier_idx + 1,
                tier.len(),
                task_names.join(", ")
            ));
        }

        for task in &mut self.tasks {
            task.is_completed = true;
        }

        Ok(execution_log)
    }
}
