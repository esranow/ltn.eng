use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemorySnapshot {
    pub project_id: String,
    pub timestamp: u64,
    pub ast_fingerprint: String,
    pub last_verified_plan_hash: String,
    pub symbol_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSwitchResult {
    pub from_project: String,
    pub to_project: String,
    pub is_cache_hit: bool,
    pub symbol_delta_count: usize,
    pub active_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossProjectMemoryEngine {
    pub snapshots: HashMap<String, ProjectMemorySnapshot>,
    pub active_project: Option<String>,
}

impl CrossProjectMemoryEngine {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            active_project: None,
        }
    }

    pub fn save_project_context(&mut self, snapshot: ProjectMemorySnapshot) {
        self.snapshots.insert(snapshot.project_id.clone(), snapshot);
    }

    pub fn get_project_context(&self, project_id: &str) -> Option<&ProjectMemorySnapshot> {
        self.snapshots.get(project_id)
    }

    pub fn switch_context(&mut self, from_project: &str, to_project: &str) -> ContextSwitchResult {
        let is_cache_hit = self.snapshots.contains_key(to_project);
        let active_fp = self
            .snapshots
            .get(to_project)
            .map(|s| s.ast_fingerprint.clone())
            .unwrap_or_else(|| "unindexed_fingerprint".to_string());

        let delta = if let (Some(from), Some(to)) = (self.snapshots.get(from_project), self.snapshots.get(to_project)) {
            (from.symbol_count as i64 - to.symbol_count as i64).unsigned_abs() as usize
        } else {
            0
        };

        self.active_project = Some(to_project.to_string());

        ContextSwitchResult {
            from_project: from_project.to_string(),
            to_project: to_project.to_string(),
            is_cache_hit,
            symbol_delta_count: delta,
            active_fingerprint: active_fp,
        }
    }
}
