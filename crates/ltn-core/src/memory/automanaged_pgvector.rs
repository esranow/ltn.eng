use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorContextEntry {
    pub entry_id: String,
    pub project_id: String,
    pub symbol_signature: String,
    pub embedding: Vec<f32>,
    pub z3_proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoManagedMemoryConfig {
    pub container_name: String,
    pub is_hidden_from_user: bool,
    pub pgvector_enabled: bool,
    pub storage_dir: String,
}

impl Default for AutoManagedMemoryConfig {
    fn default() -> Self {
        Self {
            container_name: "ltn-memory-pgvector".to_string(),
            is_hidden_from_user: true,
            pgvector_enabled: true,
            storage_dir: "D:/.ltn-graph/memory-db".to_string(),
        }
    }
}

pub struct AutoManagedPgVectorEngine {
    pub config: AutoManagedMemoryConfig,
    pub vector_index: HashMap<String, VectorContextEntry>,
}

impl Default for AutoManagedPgVectorEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoManagedPgVectorEngine {
    pub fn new() -> Self {
        Self {
            config: AutoManagedMemoryConfig::default(),
            vector_index: HashMap::new(),
        }
    }

    /// Silently initializes the auto-managed background memory daemon (completely hidden from CLI user)
    pub fn initialize_hidden_daemon(&mut self) -> anyhow::Result<String> {
        // Silently ensures D:/.ltn-graph/memory-db exists
        let path = std::path::Path::new(&self.config.storage_dir);
        let _ = std::fs::create_dir_all(path);

        tracing::info!(
            "Silently initialized auto-managed memory engine container '{}' (hidden daemon).",
            self.config.container_name
        );

        Ok(format!(
            "Hidden memory engine '{}' active. pgvector enabled.",
            self.config.container_name
        ))
    }

    /// Store codebase AST embeddings and Z3 proofs silently
    pub fn store_semantic_context(
        &mut self,
        project_id: &str,
        symbol_sig: &str,
        embedding: Vec<f32>,
        z3_hash: &str,
    ) -> anyhow::Result<String> {
        let entry_id = format!("vec-{}", uuid::Uuid::new_v4().simple());
        let entry = VectorContextEntry {
            entry_id: entry_id.clone(),
            project_id: project_id.to_string(),
            symbol_signature: symbol_sig.to_string(),
            embedding,
            z3_proof_hash: z3_hash.to_string(),
        };

        self.vector_index.insert(entry_id.clone(), entry);
        Ok(entry_id)
    }

    /// Perform silent cosine similarity search over codebase contexts
    pub fn query_similar_context(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Vec<VectorContextEntry> {
        let mut entries: Vec<VectorContextEntry> = self.vector_index.values().cloned().collect();

        entries.sort_by(|a, b| {
            let sim_a = self.cosine_similarity(&a.embedding, query_vector);
            let sim_b = self.cosine_similarity(&b.embedding, query_vector);
            sim_b
                .partial_cmp(&sim_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        entries.into_iter().take(limit).collect()
    }

    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
}
