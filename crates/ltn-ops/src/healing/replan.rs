use super::super::observer::ObservationTelemetry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RemediationTarget {
    Container(String),
    Process {
        identifier: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        respawn_command: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
}

impl RemediationTarget {
    pub fn container(name_or_id: impl Into<String>) -> Self {
        Self::Container(name_or_id.into())
    }

    pub fn process(identifier: impl Into<String>) -> Self {
        Self::Process {
            identifier: identifier.into(),
            respawn_command: None,
            args: Vec::new(),
        }
    }

    pub fn process_with_respawn(
        identifier: impl Into<String>,
        cmd: impl Into<String>,
        args: Vec<String>,
    ) -> Self {
        Self::Process {
            identifier: identifier.into(),
            respawn_command: Some(cmd.into()),
            args,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfHealingAction {
    pub drift_cause: String,
    pub corrective_action: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct ReplanEngine {
    pub default_target: Option<RemediationTarget>,
}

impl ReplanEngine {
    pub fn new(default_target: Option<RemediationTarget>) -> Self {
        Self { default_target }
    }

    pub fn heal(&self, telemetry: &ObservationTelemetry) -> Option<SelfHealingAction> {
        Self::evaluate_and_heal_target(telemetry, self.default_target.as_ref())
    }

    pub fn evaluate_and_heal(telemetry: &ObservationTelemetry) -> Option<SelfHealingAction> {
        Self::evaluate_and_heal_target(telemetry, None)
    }

    pub fn evaluate_and_heal_target(
        telemetry: &ObservationTelemetry,
        target: Option<&RemediationTarget>,
    ) -> Option<SelfHealingAction> {
        if !telemetry.drift_detected {
            return None;
        }

        let resolved_target = if let Some(t) = target {
            t.clone()
        } else if let Ok(container_id) = std::env::var("LTN_TARGET_CONTAINER") {
            RemediationTarget::container(container_id)
        } else if let Ok(proc_name) = std::env::var("LTN_TARGET_PROCESS") {
            RemediationTarget::process(proc_name)
        } else if let Some(dead_proc) = telemetry.process_probes.iter().find(|p| !p.running) {
            RemediationTarget::process(&dead_proc.name)
        } else if !telemetry.http_probes.is_empty()
            && telemetry.http_probes.iter().any(|h| h.status_code != 200)
        {
            RemediationTarget::container(
                std::env::var("LTN_DEFAULT_CONTAINER").unwrap_or_else(|_| "ltn-workload".to_string()),
            )
        } else {
            RemediationTarget::container("ltn-service".to_string())
        };

        Some(Self::execute_remediation(&resolved_target))
    }

    pub fn execute_remediation(target: &RemediationTarget) -> SelfHealingAction {
        match target {
            RemediationTarget::Container(id) => Self::restart_container(id),
            RemediationTarget::Process {
                identifier,
                respawn_command,
                args,
            } => Self::remediate_process(identifier, respawn_command.as_deref(), args),
        }
    }

    /// Attempts to restart a container using `docker restart <container_id>`.
    /// Sets success and exit_code based on actual command execution.
    pub fn restart_container(container_id: &str) -> SelfHealingAction {
        tracing::info!("Executing docker restart on container '{}'", container_id);
        let output = std::process::Command::new("docker")
            .arg("restart")
            .arg(container_id)
            .output();

        match output {
            Ok(out) => {
                let exit_code = out.status.code().unwrap_or(if out.status.success() { 0 } else { 1 });
                let success = out.status.success();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();

                let corrective_action = if success {
                    format!(
                        "Restarted container '{}' via docker restart (exit code 0)",
                        container_id
                    )
                } else {
                    let mut details = format!(
                        "Attempted docker restart '{}': exit code {}",
                        container_id, exit_code
                    );
                    if !stderr.is_empty() {
                        details.push_str(&format!(", stderr: {}", stderr));
                    }
                    if !stdout.is_empty() {
                        details.push_str(&format!(", stdout: {}", stdout));
                    }
                    details
                };

                SelfHealingAction {
                    drift_cause: format!(
                        "Container '{}' failed health check",
                        container_id
                    ),
                    corrective_action,
                    success,
                    target: Some(container_id.to_string()),
                    exit_code: Some(exit_code),
                }
            }
            Err(err) => {
                let corrective_action = format!(
                    "Failed to invoke docker restart for container '{}': {}",
                    container_id, err
                );
                SelfHealingAction {
                    drift_cause: format!(
                        "Container '{}' failed health check",
                        container_id
                    ),
                    corrective_action,
                    success: false,
                    target: Some(container_id.to_string()),
                    exit_code: Some(1),
                }
            }
        }
    }

    /// Terminates existing process instances matching identifier and respawns a fresh instance.
    /// Sets success and exit_code based on actual exit code or running process status.
    pub fn remediate_process(
        identifier: &str,
        respawn_command: Option<&str>,
        args: &[String],
    ) -> SelfHealingAction {
        tracing::info!("Remediating process target '{}'", identifier);
        let mut actions = Vec::new();

        let is_pid = identifier.parse::<u32>().is_ok();
        let mut killed_any = false;

        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();

        let mut detected_exe: Option<std::path::PathBuf> = None;
        let mut detected_cmd: Vec<String> = Vec::new();

        for (pid, proc) in sys.processes() {
            let matches = if is_pid {
                pid.as_u32().to_string() == identifier
            } else {
                proc.name().eq_ignore_ascii_case(identifier)
                    || proc.name().to_lowercase().starts_with(&identifier.to_lowercase())
            };

            if matches {
                if detected_exe.is_none() {
                    detected_exe = proc.exe().map(|p| p.to_path_buf());
                    detected_cmd = proc.cmd().to_vec();
                }
                proc.kill();
                killed_any = true;
                actions.push(format!("Terminated process PID {} ({})", pid, proc.name()));
            }
        }

        if !killed_any {
            #[cfg(windows)]
            {
                let mut cmd = std::process::Command::new("taskkill");
                cmd.arg("/F");
                if is_pid {
                    cmd.args(["/PID", identifier]);
                } else {
                    cmd.args(["/IM", identifier]);
                }
                if let Ok(out) = cmd.output() {
                    if out.status.success() {
                        actions.push(format!("Killed process '{}' via taskkill", identifier));
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let mut cmd = if is_pid {
                    let mut c = std::process::Command::new("kill");
                    c.args(["-9", identifier]);
                    c
                } else {
                    let mut c = std::process::Command::new("pkill");
                    c.args(["-9", "-f", identifier]);
                    c
                };
                if let Ok(out) = cmd.output() {
                    if out.status.success() {
                        actions.push(format!("Killed process '{}' via kill/pkill", identifier));
                    }
                }
            }
        }

        let cmd_to_run = if let Some(cmd) = respawn_command {
            Some((cmd.to_string(), args.to_vec()))
        } else if let Some(exe) = detected_exe {
            let exe_str = exe.to_string_lossy().to_string();
            let cmd_args = if !args.is_empty() {
                args.to_vec()
            } else if detected_cmd.len() > 1 {
                detected_cmd[1..].to_vec()
            } else {
                Vec::new()
            };
            Some((exe_str, cmd_args))
        } else {
            Some((identifier.to_string(), args.to_vec()))
        };

        if let Some((exec, exec_args)) = cmd_to_run {
            let mut spawn_cmd = std::process::Command::new(&exec);
            spawn_cmd.args(&exec_args);
            match spawn_cmd.spawn() {
                Ok(mut child) => {
                    let pid = child.id();
                    // Poll briefly to see if process terminates with an exit status (e.g. short-lived command)
                    let mut exit_status = None;
                    for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        if let Ok(Some(status)) = child.try_wait() {
                            exit_status = Some(status);
                            break;
                        }
                    }

                    match exit_status {
                        Some(status) => {
                            let code = status.code().unwrap_or(if status.success() { 0 } else { 1 });
                            let success = status.success();
                            actions.push(format!(
                                "Respawned '{}' which exited with code {}",
                                exec, code
                            ));
                            SelfHealingAction {
                                drift_cause: format!(
                                    "Process '{}' degraded or terminated",
                                    identifier
                                ),
                                corrective_action: actions.join("; "),
                                success,
                                target: Some(identifier.to_string()),
                                exit_code: Some(code),
                            }
                        }
                        None => {
                            // Process is running actively as a background daemon
                            actions.push(format!("Respawned '{}' with PID {}", exec, pid));
                            SelfHealingAction {
                                drift_cause: format!(
                                    "Process '{}' degraded or terminated",
                                    identifier
                                ),
                                corrective_action: actions.join("; "),
                                success: true,
                                target: Some(identifier.to_string()),
                                exit_code: Some(0),
                            }
                        }
                    }
                }
                Err(e) => {
                    actions.push(format!("Failed to respawn process '{}': {}", exec, e));
                    SelfHealingAction {
                        drift_cause: format!("Process '{}' degraded or terminated", identifier),
                        corrective_action: actions.join("; "),
                        success: false,
                        target: Some(identifier.to_string()),
                        exit_code: Some(1),
                    }
                }
            }
        } else {
            SelfHealingAction {
                drift_cause: format!("Process '{}' missing or degraded", identifier),
                corrective_action: "No executable identified to respawn process".to_string(),
                success: false,
                target: Some(identifier.to_string()),
                exit_code: Some(1),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healing_no_drift() {
        let telemetry = ObservationTelemetry {
            socket_probes: vec![],
            process_probes: vec![],
            http_probes: vec![],
            drift_detected: false,
        };
        assert!(ReplanEngine::evaluate_and_heal(&telemetry).is_none());
    }

    #[test]
    fn test_container_remediation_real_exit_code() {
        let action = ReplanEngine::restart_container("nonexistent-test-container-404");
        // Docker container does not exist or daemon is offline, verifying actual failure status
        assert!(!action.success);
        assert!(action.exit_code.is_some());
        assert_eq!(action.target, Some("nonexistent-test-container-404".to_string()));
    }

    #[test]
    fn test_process_remediation_spawn_success() {
        #[cfg(windows)]
        let (cmd, args) = ("cmd.exe", vec!["/c".to_string(), "exit 0".to_string()]);
        #[cfg(not(windows))]
        let (cmd, args) = ("sh", vec!["-c".to_string(), "exit 0".to_string()]);

        let target = RemediationTarget::process_with_respawn("test-proc", cmd, args);
        let action = ReplanEngine::execute_remediation(&target);
        assert!(action.success);
        assert_eq!(action.exit_code, Some(0));
        assert!(action.corrective_action.contains("Respawned"));
    }

    #[test]
    fn test_process_remediation_spawn_failure() {
        let target = RemediationTarget::process_with_respawn("failing-proc", "nonexistent-cmd-404-bin", vec![]);
        let action = ReplanEngine::execute_remediation(&target);
        assert!(!action.success);
        assert_eq!(action.exit_code, Some(1));
    }
}
