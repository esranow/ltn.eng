//! Thin wrapper over the `docker` CLI. A target with `docker_host` set
//! (e.g. `ssh://deploy@10.0.0.5`) is driven remotely through the same CLI.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Docker {
    bin: String,
    host: Option<String>,
    /// Directory with ltn's `ssh` wrapper, put first on PATH for `ssh://` hosts.
    ssh_shim: Option<std::path::PathBuf>,
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
            ssh_shim: None,
        }
    }

    /// Use ltn's `ssh` wrapper (see `crate::ssh`) when the host is `ssh://`.
    pub fn with_ssh_shim(mut self, dir: std::path::PathBuf) -> Self {
        self.ssh_shim = Some(dir);
        self
    }

    /// True when this client talks to an `ssh://` host.
    pub fn is_ssh(&self) -> bool {
        self.host.as_deref().map_or(false, |h| h.starts_with("ssh://"))
    }

    /// PATH for the docker child: the ssh wrapper first, only for ssh hosts.
    pub fn child_path(&self) -> Option<std::ffi::OsString> {
        let shim = self.ssh_shim.as_ref().filter(|_| self.is_ssh())?;
        let mut parts = vec![shim.clone()];
        if let Some(p) = std::env::var_os("PATH") {
            parts.extend(std::env::split_paths(&p));
        }
        std::env::join_paths(parts).ok()
    }

    /// Runs `docker <args>` with a time limit, so a stalled daemon or SSH
    /// connection yields an error instead of hanging the MCP server. Builds
    /// get `LTN_DOCKER_BUILD_TIMEOUT_S` (default 1800 s); everything else
    /// `LTN_DOCKER_TIMEOUT_S` (default 120 s).
    pub fn run(&self, args: &[&str]) -> Result<CmdResult> {
        let limit = command_timeout(args.first().copied().unwrap_or(""));
        self.run_with_timeout(args, limit)
    }

    pub fn run_with_timeout(&self, args: &[&str], limit: Duration) -> Result<CmdResult> {
        let mut cmd = Command::new(&self.bin);
        if let Some(h) = &self.host {
            cmd.arg("-H").arg(h);
        }
        if let Some(path) = self.child_path() {
            cmd.env("PATH", path);
        }
        let mut child = cmd
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("could not run `{}`: {e}. Is Docker installed?", self.bin))?;
        // Drain both pipes on threads so a chatty command cannot block on a full pipe.
        let mut out_pipe = child.stdout.take().expect("piped stdout");
        let mut err_pipe = child.stderr.take().expect("piped stderr");
        let out_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = out_pipe.read_to_end(&mut b);
            b
        });
        let err_t = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = err_pipe.read_to_end(&mut b);
            b
        });
        let start = Instant::now();
        let status = loop {
            if let Some(st) = child.try_wait()? {
                break st;
            }
            if start.elapsed() >= limit {
                let _ = child.kill();
                let _ = child.wait();
                let target = self.host.as_deref().unwrap_or("the local Docker daemon");
                bail!(
                    "`docker {}` did not finish within {} s on {target}; the daemon or connection is not responding                      (raise LTN_DOCKER_TIMEOUT_S if it is only slow)",
                    args.iter().take(2).copied().collect::<Vec<_>>().join(" "),
                    limit.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let stdout = out_t.join().unwrap_or_default();
        let stderr = err_t.join().unwrap_or_default();
        Ok(CmdResult {
            ok: status.success(),
            code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
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

/// Time limit for one docker CLI call, from the environment.
pub fn command_timeout(subcommand: &str) -> Duration {
    let (var, default) = if subcommand == "build" {
        ("LTN_DOCKER_BUILD_TIMEOUT_S", 1800)
    } else {
        ("LTN_DOCKER_TIMEOUT_S", 120)
    };
    let secs = std::env::var(var).ok().and_then(|v| v.parse::<u64>().ok()).filter(|n| *n > 0).unwrap_or(default);
    Duration::from_secs(secs)
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

#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[test]
    fn stalled_command_is_killed_with_a_clear_error() {
        #[cfg(windows)]
        let (bin, args) = ("ping", vec!["-n", "8", "127.0.0.1"]);
        #[cfg(not(windows))]
        let (bin, args) = ("sleep", vec!["8"]);
        let start = Instant::now();
        let err = Docker::new(bin, None).run_with_timeout(&args, Duration::from_secs(1)).unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
        assert!(err.to_string().contains("did not finish within 1 s"), "{err}");
    }

    #[test]
    fn fast_command_returns_output() {
        #[cfg(windows)]
        let (bin, args) = ("cmd", vec!["/C", "echo hello"]);
        #[cfg(not(windows))]
        let (bin, args) = ("echo", vec!["hello"]);
        let r = Docker::new(bin, None).run_with_timeout(&args, Duration::from_secs(20)).unwrap();
        assert!(r.ok);
        assert_eq!(r.stdout, "hello");
    }

    #[test]
    fn ssh_shim_goes_on_path_only_for_ssh_hosts() {
        let shim = std::path::PathBuf::from("ltn-shim-dir");
        let ssh = Docker::new("docker", Some("ssh://node-a")).with_ssh_shim(shim.clone());
        let first = std::env::split_paths(&ssh.child_path().unwrap()).next().unwrap();
        assert_eq!(first, shim);
        assert!(Docker::new("docker", None).with_ssh_shim(shim.clone()).child_path().is_none());
        assert!(Docker::new("docker", Some("tcp://10.0.0.5:2376")).with_ssh_shim(shim).child_path().is_none());
        assert!(Docker::new("docker", Some("ssh://node-a")).child_path().is_none());
    }

    #[test]
    fn build_gets_the_long_limit() {
        assert!(command_timeout("build") > command_timeout("info"));
    }
}
