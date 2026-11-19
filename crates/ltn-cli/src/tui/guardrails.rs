use std::collections::VecDeque;

/// Feed-Forward Feedback Loop for empirical hardware & workload metrics
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FeedbackMetric {
    pub timestamp: String,
    pub cpu_usage_pct: f32,
    pub ram_used_mb: u64,
    pub gpu_vram_mb: u64,
    pub status: String,
}

pub struct FeedbackLoop {
    history: VecDeque<FeedbackMetric>,
    max_capacity: usize,
}

impl FeedbackLoop {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_capacity),
            max_capacity,
        }
    }

    pub fn push_metric(&mut self, metric: FeedbackMetric) {
        if self.history.len() >= self.max_capacity {
            self.history.pop_front();
        }
        self.history.push_back(metric);
    }

    pub fn latest(&self) -> Option<&FeedbackMetric> {
        self.history.back()
    }
}

/// Strict Guardrails & Sandbox Containment Engine
pub struct GuardrailEnforcer;

impl GuardrailEnforcer {
    /// Validates commands to prevent destructive code deletions
    pub fn validate_command(cmd: &str) -> Result<(), String> {
        let lower = cmd.to_lowercase();
        let dangerous_patterns = [
            "rm -rf /",
            "rm -rf *",
            "del /f /s /q",
            "format c:",
            "drop database",
            "sudo rm -rf",
        ];
        for pattern in dangerous_patterns {
            if lower.contains(pattern) {
                return Err(format!(
                    "❌ GUARDRAIL VIOLATION: Dangerous command pattern detected: '{}'",
                    pattern
                ));
            }
        }
        Ok(())
    }

    /// Verifies sandbox containment bounds. Self-terminates if escape detected.
    #[allow(dead_code)]
    pub fn enforce_sandbox_containment(target_path: &str) {
        if target_path.contains("..")
            && (target_path.contains("/etc/") || target_path.contains("C:\\Windows\\System32"))
        {
            eprintln!("💥 CRITICAL CONTAINMENT ESCAPE DETECTED: Unauthorized access outside sandbox bounds: '{}'", target_path);
            eprintln!("⚡ CONTAINMENT BREACH ACTION: Self-terminating agent process.");
            std::process::exit(101); // Self-terminate
        }
    }
}
