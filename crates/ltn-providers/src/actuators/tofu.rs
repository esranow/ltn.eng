use std::path::Path;
use std::process::Command;

pub struct TofuExecutionEngine;

pub const IAC_BINARY_MISSING_ERROR: &str =
    "IAC_BINARY_MISSING: Neither 'tofu' nor 'terraform' found on PATH. Install OpenTofu to provision cloud infrastructure.";

impl TofuExecutionEngine {
    /// Checks if `tofu` or `terraform` exists on PATH.
    /// Prefers `tofu`, falls back to `terraform`.
    pub fn check_binary_available() -> Option<String> {
        for bin in &["tofu", "terraform"] {
            // First check: invoke `<bin> version`
            if let Ok(output) = Self::create_command(bin).arg("version").output() {
                if output.status.success() {
                    return Some((*bin).to_string());
                }
            }

            // Second check: search PATH directories for the binary
            if let Ok(path_var) = std::env::var("PATH") {
                let extensions = if cfg!(windows) {
                    vec![".exe", ".cmd", ".bat", ""]
                } else {
                    vec![""]
                };
                for dir in std::env::split_paths(&path_var) {
                    for ext in &extensions {
                        let candidate = dir.join(format!("{}{}", bin, ext));
                        if candidate.is_file() {
                            return Some((*bin).to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn create_command(bin: &str) -> Command {
        if cfg!(windows) {
            if let Ok(path_var) = std::env::var("PATH") {
                for dir in std::env::split_paths(&path_var) {
                    for ext in &[".cmd", ".bat"] {
                        let candidate = dir.join(format!("{}{}", bin, ext));
                        if candidate.is_file() {
                            let mut cmd = Command::new("cmd");
                            cmd.args(["/C", &candidate.to_string_lossy()]);
                            return cmd;
                        }
                    }
                }
            }
        }
        Command::new(bin)
    }

    /// Runs `tofu init -no-color` in the specified working directory.
    pub fn init(work_dir: &Path) -> Result<String, String> {
        let bin = Self::check_binary_available()
            .ok_or_else(|| IAC_BINARY_MISSING_ERROR.to_string())?;

        let output = Self::create_command(&bin)
            .current_dir(work_dir)
            .args(["init", "-no-color"])
            .output()
            .map_err(|e| format!("Failed to execute {} init: {}", bin, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            Err(format!(
                "{} init failed with exit code {:?}: {}",
                bin,
                output.status.code(),
                detail
            ))
        }
    }

    /// Runs `tofu plan -out=<out_file> -no-color` in the specified working directory.
    pub fn plan(work_dir: &Path, out_file: &str) -> Result<String, String> {
        let bin = Self::check_binary_available()
            .ok_or_else(|| IAC_BINARY_MISSING_ERROR.to_string())?;

        let out_arg = format!("-out={}", out_file);
        let output = Self::create_command(&bin)
            .current_dir(work_dir)
            .args(["plan", &out_arg, "-no-color"])
            .output()
            .map_err(|e| format!("Failed to execute {} plan: {}", bin, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            Err(format!(
                "{} plan failed with exit code {:?}: {}",
                bin,
                output.status.code(),
                detail
            ))
        }
    }

    /// Runs `tofu apply -auto-approve <plan_file>` in the specified working directory.
    pub fn apply(work_dir: &Path, plan_file: &str) -> Result<String, String> {
        let bin = Self::check_binary_available()
            .ok_or_else(|| IAC_BINARY_MISSING_ERROR.to_string())?;

        let mut cmd = Self::create_command(&bin);
        cmd.current_dir(work_dir);
        cmd.arg("apply");
        cmd.arg("-auto-approve");
        cmd.arg("-no-color");
        if !plan_file.trim().is_empty() {
            cmd.arg(plan_file);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to execute {} apply: {}", bin, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            Err(format!(
                "{} apply failed with exit code {:?}: {}",
                bin,
                output.status.code(),
                detail
            ))
        }
    }

    /// Runs `tofu destroy -auto-approve -no-color` in the specified working directory.
    pub fn destroy(work_dir: &Path) -> Result<String, String> {
        let bin = Self::check_binary_available()
            .ok_or_else(|| IAC_BINARY_MISSING_ERROR.to_string())?;

        let output = Self::create_command(&bin)
            .current_dir(work_dir)
            .args(["destroy", "-auto-approve", "-no-color"])
            .output()
            .map_err(|e| format!("Failed to execute {} destroy: {}", bin, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            let detail = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            Err(format!(
                "{} destroy failed with exit code {:?}: {}",
                bin,
                output.status.code(),
                detail
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_missing_binary_error_message() {
        let _guard = ENV_LOCK.lock().unwrap();
        if TofuExecutionEngine::check_binary_available().is_none() {
            let temp_dir = std::env::temp_dir();
            let init_err = TofuExecutionEngine::init(&temp_dir).unwrap_err();
            assert_eq!(init_err, IAC_BINARY_MISSING_ERROR);

            let plan_err = TofuExecutionEngine::plan(&temp_dir, "test.tfplan").unwrap_err();
            assert_eq!(plan_err, IAC_BINARY_MISSING_ERROR);

            let apply_err = TofuExecutionEngine::apply(&temp_dir, "test.tfplan").unwrap_err();
            assert_eq!(apply_err, IAC_BINARY_MISSING_ERROR);

            let destroy_err = TofuExecutionEngine::destroy(&temp_dir).unwrap_err();
            assert_eq!(destroy_err, IAC_BINARY_MISSING_ERROR);
        }
    }

    #[test]
    fn test_mock_binary_execution() {
        let _guard = ENV_LOCK.lock().unwrap();
        let temp_dir = std::env::temp_dir().join(format!("tofu_mock_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mock_file = if cfg!(windows) {
            temp_dir.join("tofu.bat")
        } else {
            temp_dir.join("tofu")
        };

        if cfg!(windows) {
            std::fs::write(&mock_file, "@echo off\r\necho MOCK_TOFU_SUCCESS %*\r\nexit /b 0\r\n").unwrap();
        } else {
            std::fs::write(&mock_file, "#!/bin/sh\necho \"MOCK_TOFU_SUCCESS $@\"\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&mock_file).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&mock_file, perms).unwrap();
            }
        }

        let old_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{};{}", temp_dir.display(), old_path);
        std::env::set_var("PATH", &new_path);

        let binary_found = TofuExecutionEngine::check_binary_available();
        assert_eq!(binary_found, Some("tofu".to_string()));

        let init_out = TofuExecutionEngine::init(&temp_dir);
        assert!(init_out.is_ok(), "init should succeed with mock binary: {:?}", init_out);
        assert!(init_out.unwrap().contains("MOCK_TOFU_SUCCESS init -no-color"));

        let plan_out = TofuExecutionEngine::plan(&temp_dir, "plan.out");
        assert!(plan_out.is_ok(), "plan should succeed with mock binary: {:?}", plan_out);
        assert!(plan_out.unwrap().contains("MOCK_TOFU_SUCCESS plan -out=plan.out -no-color"));

        let apply_out = TofuExecutionEngine::apply(&temp_dir, "plan.out");
        assert!(apply_out.is_ok(), "apply should succeed with mock binary: {:?}", apply_out);
        assert!(apply_out.unwrap().contains("MOCK_TOFU_SUCCESS apply -auto-approve -no-color plan.out"));

        let destroy_out = TofuExecutionEngine::destroy(&temp_dir);
        assert!(destroy_out.is_ok(), "destroy should succeed with mock binary: {:?}", destroy_out);
        assert!(destroy_out.unwrap().contains("MOCK_TOFU_SUCCESS destroy -auto-approve -no-color"));

        // Restore PATH and clean up
        std::env::set_var("PATH", old_path);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
