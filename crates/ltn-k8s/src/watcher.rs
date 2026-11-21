use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum K8sPodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sPodCondition {
    pub pod_name: String,
    pub phase: K8sPodPhase,
    pub restart_count: i32,
    pub waiting_reason: Option<String>, // e.g. "CrashLoopBackOff", "ImagePullBackOff", "OOMKilled"
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum K8sHealthVerdict {
    Healthy,
    Degraded { reason: String },
    NeedsRollback { reason: String },
}

pub struct K8sPodHealthAnalyzer;

impl K8sPodHealthAnalyzer {
    pub fn evaluate(pods: &[K8sPodCondition]) -> K8sHealthVerdict {
        if pods.is_empty() {
            return K8sHealthVerdict::Degraded {
                reason: "Zero pods found for deployment".to_string(),
            };
        }

        for pod in pods {
            if let Some(ref reason) = pod.waiting_reason {
                if reason == "CrashLoopBackOff" || reason == "OOMKilled" || pod.restart_count > 3 {
                    return K8sHealthVerdict::NeedsRollback {
                        reason: format!(
                            "Pod {} in critical state: {} (restarts: {})",
                            pod.pod_name, reason, pod.restart_count
                        ),
                    };
                }
                if reason == "ImagePullBackOff" || reason == "ErrImagePull" {
                    return K8sHealthVerdict::NeedsRollback {
                        reason: format!("Pod {} cannot pull image: {}", pod.pod_name, reason),
                    };
                }
            }
            if pod.phase == K8sPodPhase::Failed {
                return K8sHealthVerdict::NeedsRollback {
                    reason: format!("Pod {} failed execution", pod.pod_name),
                };
            }
        }

        let all_ready = pods.iter().all(|p| p.ready && p.phase == K8sPodPhase::Running);
        if all_ready {
            K8sHealthVerdict::Healthy
        } else {
            K8sHealthVerdict::Degraded {
                reason: "Pods still converging / not all ready".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthy_pods() {
        let pods = vec![
            K8sPodCondition {
                pod_name: "app-abc-1".to_string(),
                phase: K8sPodPhase::Running,
                restart_count: 0,
                waiting_reason: None,
                ready: true,
            },
            K8sPodCondition {
                pod_name: "app-abc-2".to_string(),
                phase: K8sPodPhase::Running,
                restart_count: 0,
                waiting_reason: None,
                ready: true,
            },
        ];
        assert_eq!(K8sPodHealthAnalyzer::evaluate(&pods), K8sHealthVerdict::Healthy);
    }

    #[test]
    fn test_crash_loop_triggers_rollback() {
        let pods = vec![K8sPodCondition {
            pod_name: "app-abc-crash".to_string(),
            phase: K8sPodPhase::Running,
            restart_count: 4,
            waiting_reason: Some("CrashLoopBackOff".to_string()),
            ready: false,
        }];
        match K8sPodHealthAnalyzer::evaluate(&pods) {
            K8sHealthVerdict::NeedsRollback { reason } => {
                assert!(reason.contains("CrashLoopBackOff"));
            }
            _ => panic!("Expected NeedsRollback"),
        }
    }
}
