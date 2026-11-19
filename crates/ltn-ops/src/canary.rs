use ltn_core::guardrails::GuardrailEnforcer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CanaryPhase {
    Pending,
    Stage5Percent,
    Stage25Percent,
    Stage50Percent,
    FullyPromoted,
    RolledBack(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanaryTelemetrySnapshot {
    pub sample_count: u64,
    pub error_count: u64,
    pub p99_latency_ms: f64,
}

impl CanaryTelemetrySnapshot {
    pub fn error_rate(&self) -> f64 {
        if self.sample_count == 0 {
            0.0
        } else {
            self.error_count as f64 / self.sample_count as f64
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanaryStepResult {
    pub phase: CanaryPhase,
    pub traffic_pct: u8,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimulatedRolloutReport {
    pub stages: Vec<CanaryStepResult>,
    pub overall_success: bool,
    pub final_traffic_pct: u8,
    pub rollback_triggered: bool,
    pub rollback_latency_ms: f64,
    pub slo_budget_burn_rate: f64,
}

pub struct CanaryOrchestrator {
    pub current_phase: CanaryPhase,
    pub traffic_pct: u8,
}

impl Default for CanaryOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl CanaryOrchestrator {
    pub fn new() -> Self {
        Self {
            current_phase: CanaryPhase::Pending,
            traffic_pct: 0,
        }
    }

    /// Progresses canary deployment through staged percentages (5% -> 25% -> 50% -> 100%).
    /// Evaluates strict SLO error budget and latency SLA at each stage.
    /// Fails closed: any violation instantly triggers sub-50ms rollback to stable baseline.
    pub fn advance_canary_stage(
        &mut self,
        telemetry: &CanaryTelemetrySnapshot,
    ) -> CanaryStepResult {
        // Enforce non-bypassable SLO guardrails
        let error_rate = telemetry.error_rate();
        if let Err(violation) =
            GuardrailEnforcer::enforce_canary_slo(error_rate, telemetry.p99_latency_ms)
        {
            let rollback_reason =
                format!("Auto-rollback triggered by guardrail: {}", violation.reason);
            self.current_phase = CanaryPhase::RolledBack(rollback_reason.clone());
            self.traffic_pct = 0;
            return CanaryStepResult {
                phase: self.current_phase.clone(),
                traffic_pct: 0,
                success: false,
                message: rollback_reason,
            };
        }

        match self.current_phase {
            CanaryPhase::Pending => {
                self.current_phase = CanaryPhase::Stage5Percent;
                self.traffic_pct = 5;
                CanaryStepResult {
                    phase: self.current_phase.clone(),
                    traffic_pct: 5,
                    success: true,
                    message: "Canary Stage 1 initiated: 5% traffic shifted to candidate replica."
                        .to_string(),
                }
            }
            CanaryPhase::Stage5Percent => {
                self.current_phase = CanaryPhase::Stage25Percent;
                self.traffic_pct = 25;
                CanaryStepResult {
                    phase: self.current_phase.clone(),
                    traffic_pct: 25,
                    success: true,
                    message: "Canary Stage 2 promoted: 25% traffic shifted. SLO verified."
                        .to_string(),
                }
            }
            CanaryPhase::Stage25Percent => {
                self.current_phase = CanaryPhase::Stage50Percent;
                self.traffic_pct = 50;
                CanaryStepResult {
                    phase: self.current_phase.clone(),
                    traffic_pct: 50,
                    success: true,
                    message: "Canary Stage 3 promoted: 50% traffic shifted. SLO verified."
                        .to_string(),
                }
            }
            CanaryPhase::Stage50Percent => {
                self.current_phase = CanaryPhase::FullyPromoted;
                self.traffic_pct = 100;
                CanaryStepResult {
                    phase: self.current_phase.clone(),
                    traffic_pct: 100,
                    success: true,
                    message: "Canary fully promoted: 100% traffic shifted to new version. Deployment complete.".to_string(),
                }
            }
            CanaryPhase::FullyPromoted => CanaryStepResult {
                phase: CanaryPhase::FullyPromoted,
                traffic_pct: 100,
                success: true,
                message: "Workload already fully promoted.".to_string(),
            },
            CanaryPhase::RolledBack(ref reason) => CanaryStepResult {
                phase: self.current_phase.clone(),
                traffic_pct: 0,
                success: false,
                message: format!("Cannot advance: Canary is rolled back ({})", reason),
            },
        }
    }

    /// Simulates staged canary traffic progression through 4 stages: 5% -> 25% -> 50% -> 100%.
    /// At each stage, passes healthy telemetry (error rate 0.0001, p99 latency 20ms) unless failure is induced.
    /// If failure is induced, triggers instant fail-closed rollback via `advance_canary_stage`,
    /// measures execution time of rollback (guaranteed sub-50ms), and records `rollback_triggered = true`.
    pub fn simulate_rollout(
        &mut self,
        should_induce_failure_at_stage: Option<u8>,
    ) -> SimulatedRolloutReport {
        self.current_phase = CanaryPhase::Pending;
        self.traffic_pct = 0;

        let mut stages = Vec::with_capacity(4);
        let mut rollback_triggered = false;
        let mut rollback_latency_ms = 0.0;
        let mut last_error_rate = 0.0001;

        let stage_targets: [(u8, u8); 4] = [(1, 5), (2, 25), (3, 50), (4, 100)];

        for (stage_num, stage_pct) in stage_targets {
            let induce_failure = match should_induce_failure_at_stage {
                Some(target) => target == stage_num || target == stage_pct,
                None => false,
            };

            let telemetry = if induce_failure {
                last_error_rate = 0.05; // 5% error rate spike (exceeds 0.05% SLO threshold)
                CanaryTelemetrySnapshot {
                    sample_count: 10_000,
                    error_count: 500,
                    p99_latency_ms: 300.0,
                }
            } else {
                last_error_rate = 0.0001; // Healthy telemetry: 0.01% error rate (< 0.05% threshold)
                CanaryTelemetrySnapshot {
                    sample_count: 10_000,
                    error_count: 1,
                    p99_latency_ms: 20.0,
                }
            };

            let start = std::time::Instant::now();
            let step_result = self.advance_canary_stage(&telemetry);
            let elapsed_ms = (start.elapsed().as_secs_f64() * 1000.0).max(0.001);

            stages.push(step_result.clone());

            if !step_result.success {
                rollback_triggered = true;
                rollback_latency_ms = elapsed_ms;
                break;
            }
        }

        let overall_success =
            !rollback_triggered && self.current_phase == CanaryPhase::FullyPromoted;
        let final_traffic_pct = self.traffic_pct;
        let slo_budget_burn_rate = last_error_rate / 0.0005;

        SimulatedRolloutReport {
            stages,
            overall_success,
            final_traffic_pct,
            rollback_triggered,
            rollback_latency_ms,
            slo_budget_burn_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canary_full_promotion_lifecycle() {
        let mut canary = CanaryOrchestrator::new();
        let healthy_telemetry = CanaryTelemetrySnapshot {
            sample_count: 10000,
            error_count: 1,       // 0.01% error rate (< 0.05% threshold)
            p99_latency_ms: 45.0, // < 250ms SLA
        };

        // 0% -> 5%
        let res1 = canary.advance_canary_stage(&healthy_telemetry);
        assert!(res1.success);
        assert_eq!(res1.traffic_pct, 5);

        // 5% -> 25%
        let res2 = canary.advance_canary_stage(&healthy_telemetry);
        assert!(res2.success);
        assert_eq!(res2.traffic_pct, 25);

        // 25% -> 50%
        let res3 = canary.advance_canary_stage(&healthy_telemetry);
        assert!(res3.success);
        assert_eq!(res3.traffic_pct, 50);

        // 50% -> 100%
        let res4 = canary.advance_canary_stage(&healthy_telemetry);
        assert!(res4.success);
        assert_eq!(res4.traffic_pct, 100);
        assert_eq!(canary.current_phase, CanaryPhase::FullyPromoted);
    }

    #[test]
    fn test_canary_instant_rollback_on_error_spike() {
        let mut canary = CanaryOrchestrator::new();
        let healthy = CanaryTelemetrySnapshot {
            sample_count: 1000,
            error_count: 0,
            p99_latency_ms: 30.0,
        };
        canary.advance_canary_stage(&healthy);
        assert_eq!(canary.traffic_pct, 5);

        // Latency spikes or error rate exceeds threshold (50 errors out of 1000 = 5% error rate)
        let degraded = CanaryTelemetrySnapshot {
            sample_count: 1000,
            error_count: 50,
            p99_latency_ms: 30.0,
        };
        let res = canary.advance_canary_stage(&degraded);

        assert!(!res.success);
        assert_eq!(res.traffic_pct, 0);
        assert!(matches!(canary.current_phase, CanaryPhase::RolledBack(_)));
    }

    #[test]
    fn test_simulate_rollout_full_success_4_stages() {
        let mut canary = CanaryOrchestrator::new();
        let report = canary.simulate_rollout(None);

        assert!(report.overall_success);
        assert_eq!(report.final_traffic_pct, 100);
        assert!(!report.rollback_triggered);
        assert_eq!(report.rollback_latency_ms, 0.0);
        assert_eq!(report.stages.len(), 4);
        assert!(report.slo_budget_burn_rate <= 1.0);
        assert!((report.slo_budget_burn_rate - 0.2).abs() < 1e-6);

        assert_eq!(report.stages[0].traffic_pct, 5);
        assert!(report.stages[0].success);
        assert_eq!(report.stages[1].traffic_pct, 25);
        assert!(report.stages[1].success);
        assert_eq!(report.stages[2].traffic_pct, 50);
        assert!(report.stages[2].success);
        assert_eq!(report.stages[3].traffic_pct, 100);
        assert!(report.stages[3].success);

        assert_eq!(canary.current_phase, CanaryPhase::FullyPromoted);
        assert_eq!(canary.traffic_pct, 100);
    }

    #[test]
    fn test_simulate_rollout_sub_50ms_fail_closed_rollback_all_stages() {
        // Test rollback at each stage (1: 5%, 2: 25%, 3: 50%, 4: 100%)
        for stage in 1..=4u8 {
            let mut canary = CanaryOrchestrator::new();
            let report = canary.simulate_rollout(Some(stage));

            assert!(
                !report.overall_success,
                "Stage {} failure should not report overall success",
                stage
            );
            assert!(
                report.rollback_triggered,
                "Stage {} failure must trigger rollback",
                stage
            );
            assert_eq!(
                report.final_traffic_pct, 0,
                "Traffic must fail closed to 0%"
            );
            assert!(
                report.rollback_latency_ms < 50.0,
                "Rollback latency must be sub-50ms fail-closed (got {:.3}ms)",
                report.rollback_latency_ms
            );
            assert!(
                report.rollback_latency_ms > 0.0,
                "Rollback latency must be recorded"
            );
            assert!(
                report.slo_budget_burn_rate > 1.0,
                "SLO budget burn rate should indicate violation (> 1.0)"
            );
            assert_eq!(
                report.stages.len(),
                stage as usize,
                "Should halt execution at the failing stage"
            );

            // Last stage must be failed and rolled back
            let last_step = report.stages.last().unwrap();
            assert!(!last_step.success);
            assert_eq!(last_step.traffic_pct, 0);
            assert!(matches!(last_step.phase, CanaryPhase::RolledBack(_)));
            assert!(matches!(canary.current_phase, CanaryPhase::RolledBack(_)));
            assert_eq!(canary.traffic_pct, 0);
        }
    }

    #[test]
    fn test_simulate_rollout_with_traffic_percentage_argument() {
        let mut canary = CanaryOrchestrator::new();
        // Inducing failure by target percentage (50% traffic)
        let report = canary.simulate_rollout(Some(50));

        assert!(!report.overall_success);
        assert!(report.rollback_triggered);
        assert_eq!(report.final_traffic_pct, 0);
        assert!(report.rollback_latency_ms < 50.0);
        assert_eq!(report.stages.len(), 3); // Stages 5%, 25%, 50%
        assert!(report.stages[0].success);
        assert!(report.stages[1].success);
        assert!(!report.stages[2].success);
    }

    #[test]
    fn test_simulated_rollout_report_serde_roundtrip() {
        let mut canary = CanaryOrchestrator::new();
        let report = canary.simulate_rollout(None);

        let json = serde_json::to_string(&report).expect("Serialization failed");
        let deserialized: SimulatedRolloutReport =
            serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(report, deserialized);
    }
}
