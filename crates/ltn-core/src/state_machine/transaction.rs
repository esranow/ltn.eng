use crate::crypto::StateHasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub timestamp: String,
    pub state_hash: String,
    pub data: HashMap<String, String>,
}

pub struct StateMachine {
    current_state: HashMap<String, String>,
    pub history: Vec<StateSnapshot>,
    pub audit_log: Vec<String>,
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
            Some(previous)
        } else {
            None
        }
    }
}
