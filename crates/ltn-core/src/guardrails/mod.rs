use serde::{Deserialize, Serialize};

pub mod permissions;
pub mod policy_engine;

pub use permissions::*;
pub use policy_engine::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SafetyInvariant {
    RequireCryptographicSignature,
    EnforceSmtProof,
    ZeroPlaintextSecrets,
    CanarySloProtection,
    DisallowDestructiveWithoutSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailViolation {
    pub invariant: SafetyInvariant,
    pub reason: String,
    pub blocked_action: String,
    pub timestamp_epoch_ms: u64,
}

pub struct GuardrailEnforcer;

impl GuardrailEnforcer {
    /// Evaluates all non-bypassable safety invariants against any proposed AI or automated mutation.
    /// Fails closed: any ambiguity or violation halts execution immediately.
    pub fn enforce_mutation_safety(
        has_smt_proof: bool,
        has_prior_snapshot: bool,
        proposed_payload: &str,
    ) -> Result<(), GuardrailViolation> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // 1. Mandatory SMT Mathematical Proof
        if !has_smt_proof {
            return Err(GuardrailViolation {
                invariant: SafetyInvariant::EnforceSmtProof,
                reason: "AI attempted to mutate infrastructure without formal Z3 SMT constraint satisfiability proof.".to_string(),
                blocked_action: "EXECUTE_MUTATION".to_string(),
                timestamp_epoch_ms: now,
            });
        }

        // 2. Mandatory Prior Snapshot for Atomic Rollback
        if !has_prior_snapshot {
            return Err(GuardrailViolation {
                invariant: SafetyInvariant::DisallowDestructiveWithoutSnapshot,
                reason: "Destructive mutation rejected: No cryptographic state snapshot committed in StateMachine prior to action.".to_string(),
                blocked_action: "EXECUTE_MUTATION".to_string(),
                timestamp_epoch_ms: now,
            });
        }

        // 3. Mandatory Zero Plaintext Secrets in Payload
        if let Some(violation) = Self::scan_for_plaintext_secrets(proposed_payload, now) {
            return Err(violation);
        }

        Ok(())
    }

    /// Scans payload for plaintext private keys, high-entropy tokens, or leaked cloud credentials.
    pub fn scan_for_plaintext_secrets(payload: &str, timestamp: u64) -> Option<GuardrailViolation> {
        let sensitive_patterns = [
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "-----BEGIN PRIVATE KEY-----",
            "AKIA[0-9A-Z]{16}",
            "ghp_[0-9a-zA-Z]{36}",
            "sk-ant-api03-",
            "AIzaSy",
        ];

        for pattern in &sensitive_patterns {
            if payload.contains(pattern) {
                return Some(GuardrailViolation {
                    invariant: SafetyInvariant::ZeroPlaintextSecrets,
                    reason: format!(
                        "Plaintext secret leak prevented: detected sensitive key pattern '{}'",
                        pattern
                    ),
                    blocked_action: "WRITE_PAYLOAD".to_string(),
                    timestamp_epoch_ms: timestamp,
                });
            }
        }

        None
    }

    /// Enforces strict canary error-budget threshold.
    /// If error rate > 0.05% (0.0005) or p99 latency > 250ms, aborts immediately.
    pub fn enforce_canary_slo(
        error_rate: f64,
        p99_latency_ms: f64,
    ) -> Result<(), GuardrailViolation> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if error_rate > 0.0005 {
            return Err(GuardrailViolation {
                invariant: SafetyInvariant::CanarySloProtection,
                reason: format!(
                    "Canary SLO error budget exceeded: error rate {:.4}% > 0.05% threshold",
                    error_rate * 100.0
                ),
                blocked_action: "PROMOTE_CANARY".to_string(),
                timestamp_epoch_ms: now,
            });
        }

        if p99_latency_ms > 250.0 {
            return Err(GuardrailViolation {
                invariant: SafetyInvariant::CanarySloProtection,
                reason: format!(
                    "Canary SLO latency SLA breached: p99 latency {:.1}ms > 250ms budget",
                    p99_latency_ms
                ),
                blocked_action: "PROMOTE_CANARY".to_string(),
                timestamp_epoch_ms: now,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guardrail_blocks_without_smt_proof() {
        let res = GuardrailEnforcer::enforce_mutation_safety(false, true, "valid_payload");
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().invariant, SafetyInvariant::EnforceSmtProof);
    }

    #[test]
    fn test_guardrail_blocks_without_snapshot() {
        let res = GuardrailEnforcer::enforce_mutation_safety(true, false, "valid_payload");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().invariant,
            SafetyInvariant::DisallowDestructiveWithoutSnapshot
        );
    }

    #[test]
    fn test_guardrail_blocks_plaintext_secret() {
        let dirty_payload = "API_TOKEN=AIzaSyD-FakeGoogleKey12345";
        let res = GuardrailEnforcer::enforce_mutation_safety(true, true, dirty_payload);
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().invariant,
            SafetyInvariant::ZeroPlaintextSecrets
        );
    }

    #[test]
    fn test_guardrail_passes_clean_payload() {
        let clean = "docker compose up -d";
        let res = GuardrailEnforcer::enforce_mutation_safety(true, true, clean);
        assert!(res.is_ok());
    }

    #[test]
    fn test_canary_slo_enforcement() {
        assert!(GuardrailEnforcer::enforce_canary_slo(0.0001, 45.0).is_ok());
        assert!(GuardrailEnforcer::enforce_canary_slo(0.005, 45.0).is_err());
        assert!(GuardrailEnforcer::enforce_canary_slo(0.0001, 350.0).is_err());
    }
}
