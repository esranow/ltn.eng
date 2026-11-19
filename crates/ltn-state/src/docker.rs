//! Thin wrapper over the `docker` CLI. A target with `docker_host` set
//! (e.g. `ssh://deploy@10.0.0.5`) is driven remotely through the same CLI.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Docker {
    bin: String,
    host: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CmdResult {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DockerInfo {
    pub cpus: u64,
    pub memory_mb: u64,
    pub server_version: String,
    pub os: String,
    pub has_nvidia_runtime: bool,
}

impl Docker {
    pub fn new(bin: &str, host: Option<&str>) -> Self {
        Self {
            bin: bin.to_string(),
            host: host.map(str::to_string),
        }
    }

    pub fn run(&self, args: &[&str]) -> Result<CmdResult> {
        let mut cmd = Command::new(&self.bin);
        if let Some(h) = &self.host {
            cmd.arg("-H").arg(h);
        }
        let out = cmd
            .args(args)
            .output()
            .map_err(|e| anyhow!("could not run `{}`: {e}. Is Docker installed?", self.bin))?;
        Ok(CmdResult {
            ok: out.status.success(),
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }

    /// Runs a command and returns stdout, or an error carrying docker's stderr.
    pub fn run_ok(&self, args: &[&str]) -> Result<String> {
        let r = self.run(args)?;
        if r.ok {
            Ok(r.stdout)
        } else {
            bail!(
                "`docker {}` failed: {}",
                args.iter().take(2).copied().collect::<Vec<_>>().join(" "),
                tail_lines(&r.stderr, 15)
            )
        }
    }

    pub fn info(&self) -> Result<DockerInfo> {
        let raw = self.run_ok(&["info", "--format", "{{json .}}"])?;
        let v: Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("unreadable `docker info` output: {e}"))?;
        if let Some(errs) = v.get("ServerErrors").and_then(|e| e.as_array()) {
            if !errs.is_empty() {
                bail!("Docker daemon not reachable: {}", errs[0]);
            }
        }
        let cpus = v["NCPU"].as_u64().unwrap_or(0);
        if cpus == 0 {
            bail!("Docker daemon not reachable (docker info reported 0 CPUs)");
        }
        Ok(DockerInfo {
            cpus,
            memory_mb: v["MemTotal"].as_u64().unwrap_or(0) / (1024 * 1024),
            server_version: v["ServerVersion"].as_str().unwrap_or("").to_string(),
            os: v["OperatingSystem"].as_str().unwrap_or("").to_string(),
            has_nvidia_runtime: v["Runtimes"].get("nvidia").is_some(),
        })
    }

    /// `docker inspect` of one container, or None if it does not exist.
    pub fn inspect(&self, name: &str) -> Result<Option<Value>> {
        let r = self.run(&["inspect", "--type", "container", name])?;
        if !r.ok {
            if r.stderr.contains("No such") || r.stderr.contains("no such") {
                return Ok(None);
            }
            bail!("`docker inspect {name}` failed: {}", tail_lines(&r.stderr, 5));
        }
        let v: Value = serde_json::from_str(&r.stdout)?;
        Ok(v.as_array().and_then(|a| a.first().cloned()))
    }

    /// stdout and stderr of a container, interleaved by docker.
    pub fn logs(&self, name: &str, tail: usize) -> Result<String> {
        let r = self.run(&["logs", "--tail", &tail.to_string(), name])?;
        if !r.ok {
            bail!("`docker logs {name}` failed: {}", tail_lines(&r.stderr, 5));
        }
        Ok(match (r.stdout.is_empty(), r.stderr.is_empty()) {
            (false, false) => format!("{}\n{}", r.stdout, r.stderr),
            (false, true) => r.stdout,
            _ => r.stderr,
        })
    }

    pub fn stats(&self, name: &str) -> Result<Option<Value>> {
        let r = self.run(&["stats", "--no-stream", "--format", "{{json .}}", name])?;
        if !r.ok {
            return Ok(None);
        }
        Ok(serde_json::from_str(&r.stdout).ok())
    }

    pub fn image_exists(&self, image: &str) -> bool {
        self.run(&["image", "inspect", image]).map(|r| r.ok).unwrap_or(false)
    }

    /// Names of every container (any state) carrying `label`.
    pub fn containers_with_label(&self, label: &str) -> Result<Vec<String>> {
        let out = self.run_ok(&["ps", "-a", "--filter", &format!("label={label}"), "--format", "{{.Names}}"])?;
        Ok(out.lines().map(str::to_string).filter(|s| !s.is_empty()).collect())
    }
}

pub fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Fields of `docker inspect` that matter for status and diagnosis.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContainerState {
    pub status: String,
    pub running: bool,
    pub restarting: bool,
    pub exit_code: i64,
    pub oom_killed: bool,
    pub restart_count: i64,
    pub started_at: String,
    pub finished_at: String,
    pub error: String,
}

impl ContainerState {
    pub fn from_inspect(v: &Value) -> Self {
        let s = &v["State"];
        Self {
            status: s["Status"].as_str().unwrap_or("unknown").to_string(),
            running: s["Running"].as_bool().unwrap_or(false),
            restarting: s["Restarting"].as_bool().unwrap_or(false),
            exit_code: s["ExitCode"].as_i64().unwrap_or(0),
            oom_killed: s["OOMKilled"].as_bool().unwrap_or(false),
            restart_count: v["RestartCount"].as_i64().unwrap_or(0),
            started_at: s["StartedAt"].as_str().unwrap_or("").to_string(),
            finished_at: s["FinishedAt"].as_str().unwrap_or("").to_string(),
            error: s["Error"].as_str().unwrap_or("").to_string(),
        }
    }
}
