use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanaryConfig {
    pub stages: Vec<u8>, // e.g. [5, 25, 50, 100]
    pub max_error_rate_delta: f64, // e.g. 0.005 = 0.5%
    pub max_p99_latency_ms: f64,   // e.g. 250.0 ms
    pub min_requests_to_evaluate: u64, // e.g. 100
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            stages: vec![5, 25, 50, 100],
            max_error_rate_delta: 0.005, // 0.5% max increase
            max_p99_latency_ms: 300.0,
            min_requests_to_evaluate: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CanaryStatus {
    Pending,
    InProgress { current_stage_idx: usize, current_weight: u8 },
    Promoted,
    Aborted { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrafficMetrics {
    pub total_requests: u64,
    pub error_requests: u64,
    pub p99_latency_ms: f64,
}

impl TrafficMetrics {
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.error_requests as f64 / self.total_requests as f64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanaryDecision {
    ContinueCurrent { weight: u8 },
    AdvanceToStage { stage_idx: usize, weight: u8 },
    PromoteComplete,
    Abort { reason: String },
}

pub struct CanaryController;

impl CanaryController {
    pub fn evaluate(
        cfg: &CanaryConfig,
        current_stage_idx: usize,
        baseline: &TrafficMetrics,
        canary: &TrafficMetrics,
    ) -> CanaryDecision {
        if canary.total_requests < cfg.min_requests_to_evaluate {
            let weight = cfg.stages[current_stage_idx];
            return CanaryDecision::ContinueCurrent { weight };
        }

        let baseline_err = baseline.error_rate();
        let canary_err = canary.error_rate();
        let delta = canary_err - baseline_err;

        if delta > cfg.max_error_rate_delta {
            return CanaryDecision::Abort {
                reason: format!(
                    "Canary error rate ({:.2}%) exceeded baseline ({:.2}%) by delta {:.2}% > limit {:.2}%",
                    canary_err * 100.0,
                    baseline_err * 100.0,
                    delta * 100.0,
                    cfg.max_error_rate_delta * 100.0
                ),
            };
        }

        if canary.p99_latency_ms > cfg.max_p99_latency_ms {
            return CanaryDecision::Abort {
                reason: format!(
                    "Canary p99 latency ({:.1}ms) exceeded threshold ({:.1}ms)",
                    canary.p99_latency_ms, cfg.max_p99_latency_ms
                ),
            };
        }

        if current_stage_idx + 1 >= cfg.stages.len() {
            CanaryDecision::PromoteComplete
        } else {
            let next_idx = current_stage_idx + 1;
            let next_weight = cfg.stages[next_idx];
            CanaryDecision::AdvanceToStage {
                stage_idx: next_idx,
                weight: next_weight,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canary_happy_progression() {
        let cfg = CanaryConfig::default();
        let baseline = TrafficMetrics {
            total_requests: 1000,
            error_requests: 2, // 0.2%
            p99_latency_ms: 45.0,
        };
        let canary = TrafficMetrics {
            total_requests: 100,
            error_requests: 0, // 0.0%
            p99_latency_ms: 48.0,
        };

        // Stage 0 (5%) -> advances to Stage 1 (25%)
        let decision = CanaryController::evaluate(&cfg, 0, &baseline, &canary);
        assert_eq!(
            decision,
            CanaryDecision::AdvanceToStage {
                stage_idx: 1,
                weight: 25
            }
        );

        // Final stage advances to promote
        let decision_final = CanaryController::evaluate(&cfg, 3, &baseline, &canary);
        assert_eq!(decision_final, CanaryDecision::PromoteComplete);
    }

    #[test]
    fn test_canary_aborts_on_error_spike() {
        let cfg = CanaryConfig::default();
        let baseline = TrafficMetrics {
            total_requests: 1000,
            error_requests: 1, // 0.1%
            p99_latency_ms: 30.0,
        };
        let canary_bad = TrafficMetrics {
            total_requests: 100,
            error_requests: 5, // 5% errors! (delta 4.9% > 0.5% limit)
            p99_latency_ms: 35.0,
        };

        let decision = CanaryController::evaluate(&cfg, 0, &baseline, &canary_bad);
        match decision {
            CanaryDecision::Abort { reason } => {
                assert!(reason.contains("Canary error rate"));
            }
            _ => panic!("Expected CanaryDecision::Abort"),
        }
    }

    #[test]
    fn test_canary_aborts_on_latency_spike() {
        let cfg = CanaryConfig::default();
        let baseline = TrafficMetrics {
            total_requests: 1000,
            error_requests: 0,
            p99_latency_ms: 40.0,
        };
        let canary_slow = TrafficMetrics {
            total_requests: 100,
            error_requests: 0,
            p99_latency_ms: 500.0, // > 300ms limit
        };

        let decision = CanaryController::evaluate(&cfg, 0, &baseline, &canary_slow);
        match decision {
            CanaryDecision::Abort { reason } => {
                assert!(reason.contains("p99 latency"));
            }
            _ => panic!("Expected CanaryDecision::Abort for latency"),
        }
    }
}
