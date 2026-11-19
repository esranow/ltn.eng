use crate::crypto::StateHasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum MutationState {
    PREPARED,
    SNAPSHOTTED,
    VALIDATED,
    EXECUTED,
    OBSERVED,
    COMMITTED,
    ROLLED_BACK,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MutationRecord {
    pub mutation_id: u64,
    pub state: MutationState,
    pub snapshot_hash: String,
    pub plan_summary: String,
    pub validation_status: String,
    pub policy_status: String,
    pub canary_status: String,
    pub verification_status: String,
    pub details: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateSnapshot {
    pub timestamp: String,
    pub state_hash: String,
    pub data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    current_state: HashMap<String, String>,
    pub history: Vec<StateSnapshot>,
    pub audit_log: Vec<String>,
    pub mutations: Vec<MutationRecord>,
    next_mutation_id: u64,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl StateMachine {
    pub fn new() -> Self {
        let current_state = HashMap::new();
        let initial_hash = StateHasher::hash_json(&current_state);
        let snapshot = StateSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            state_hash: initial_hash,
            data: current_state.clone(),
        };

        Self {
            current_state,
            history: vec![snapshot],
            audit_log: Vec::new(),
            mutations: Vec::new(),
            next_mutation_id: 1,
        }
    }

    pub fn current_hash(&self) -> String {
        StateHasher::hash_json(&self.current_state)
    }

    pub fn set_key(&mut self, key: impl Into<String>, val: impl Into<String>) {
        let k = key.into();
        let v = val.into();
        self.audit_log.push(format!("SET {}={}", k, v));
        self.current_state.insert(k, v);
    }

    pub fn get_key(&self, key: &str) -> Option<&String> {
        self.current_state.get(key)
    }

    pub fn commit(&mut self) -> StateSnapshot {
        let hash = self.current_hash();
        self.audit_log.push(format!("COMMIT hash={}", hash));
        let snapshot = StateSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            state_hash: hash,
            data: self.current_state.clone(),
        };
        self.history.push(snapshot.clone());
        snapshot
    }

    pub fn rollback(&mut self) -> Option<StateSnapshot> {
        if self.history.len() > 1 {
            self.history.pop();
            let previous = self.history.last().unwrap().clone();
            self.current_state = previous.data.clone();
            self.audit_log
                .push(format!("ROLLBACK to hash={}", previous.state_hash));
            Some(previous)
        } else {
            None
        }
    }

    /// Begin a new transactional mutation lifecycle
    pub fn begin_mutation(&mut self, plan_summary: &str) -> u64 {
        let id = self.next_mutation_id;
        self.next_mutation_id += 1;
        let snap_hash = self.current_hash();
        let now = chrono::Utc::now().to_rfc3339();

        let record = MutationRecord {
            mutation_id: id,
            state: MutationState::PREPARED,
            snapshot_hash: snap_hash,
            plan_summary: plan_summary.to_string(),
            validation_status: "PENDING".to_string(),
            policy_status: "PENDING".to_string(),
            canary_status: "PENDING".to_string(),
            verification_status: "PENDING".to_string(),
            details: vec![format!("Mutation #{} prepared: {}", id, plan_summary)],
            created_at: now.clone(),
            updated_at: now,
        };

        self.mutations.push(record);
        self.audit_log.push(format!("MUTATION_PREPARE id={}", id));
        id
    }

    /// Advance a mutation to a new state with audit details
    pub fn advance_mutation(
        &mut self,
        mutation_id: u64,
        new_state: MutationState,
        detail: &str,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(m) = self
            .mutations
            .iter_mut()
            .find(|m| m.mutation_id == mutation_id)
        {
            m.state = new_state.clone();
            m.updated_at = now;
            m.details.push(format!("[{:?}] {}", new_state, detail));
            self.audit_log.push(format!(
                "MUTATION_TRANSITION id={} state={:?}",
                mutation_id, new_state
            ));
            Ok(())
        } else {
            Err(format!(
                "Mutation #{} not found in state machine",
                mutation_id
            ))
        }
    }

    pub fn get_mutation(&self, mutation_id: u64) -> Option<&MutationRecord> {
        self.mutations.iter().find(|m| m.mutation_id == mutation_id)
    }

    /// Persist complete state machine to JSON file
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, serialized)
    }

    /// Load state machine from JSON file or initialize default
    pub fn load_from_file(path: &Path) -> Self {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(sm) = serde_json::from_str::<Self>(&content) {
                    return sm;
                }
            }
        }
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transactional_mutation_lifecycle() {
        let mut sm = StateMachine::new();
        let m_id = sm.begin_mutation("Deploy Node Postgres API to AWS ECS");
        assert_eq!(m_id, 1);

        let m = sm.get_mutation(1).unwrap();
        assert_eq!(m.state, MutationState::PREPARED);

        sm.advance_mutation(1, MutationState::SNAPSHOTTED, "State snapshot captured")
            .unwrap();
        sm.advance_mutation(1, MutationState::VALIDATED, "Z3 SMT SAT proven")
            .unwrap();
        sm.advance_mutation(
            1,
            MutationState::EXECUTED,
            "Container task definition registered",
        )
        .unwrap();
        sm.advance_mutation(1, MutationState::OBSERVED, "Canary 100% traffic healthy")
            .unwrap();
        sm.advance_mutation(1, MutationState::COMMITTED, "State committed")
            .unwrap();

        let final_m = sm.get_mutation(1).unwrap();
        assert_eq!(final_m.state, MutationState::COMMITTED);
        assert_eq!(final_m.details.len(), 6);
    }
}
