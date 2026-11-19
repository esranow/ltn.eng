//! Running external binaries (aws, tofu, ssh-keygen, curl, docker) and
//! locating them. Every binary path is injectable so tests can use fakes.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Out {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    /// Last lines of stderr (or stdout when stderr is empty), for error messages.
    pub fn evidence(&self) -> String {
        let src = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        let lines: Vec<&str> = src.lines().collect();
        let start = lines.len().saturating_sub(20);
        lines[start..].join("\n")
    }
}

/// Runs `bin args...` (optionally in `cwd`). `Err` only when the binary could not be started.
pub fn run(bin: &str, args: &[&str], cwd: Option<&Path>) -> Result<Out> {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().map_err(|e| anyhow!("could not run `{bin}`: {e}"))?;
    Ok(Out {
        ok: out.status.success(),
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    })
}

/// Finds `name` on PATH (honouring PATHEXT on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    for dir in std::env::split_paths(&path) {
        let plain = dir.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The external binaries the AWS provisioner drives.
#[derive(Debug, Clone)]
pub struct Bins {
    pub aws: String,
    /// None when neither `LTN_TOFU_BIN`, `tofu` nor `terraform` is available.
    pub tofu: Option<String>,
    pub ssh_keygen: String,
    pub curl: String,
    /// None = use `Ctx::docker_bin`.
    pub docker: Option<String>,
}

impl Bins {
    /// `LTN_AWS_BIN` (default `aws`), `LTN_TOFU_BIN` (else `tofu`, else
    /// `terraform` on PATH), `LTN_SSH_KEYGEN_BIN` (default `ssh-keygen`),
    /// `LTN_CURL_BIN` (default `curl`).
    pub fn from_env() -> Self {
        Self {
            aws: aws_bin(),
            tofu: tofu_bin(),
            ssh_keygen: env_or("LTN_SSH_KEYGEN_BIN", "ssh-keygen"),
            curl: env_or("LTN_CURL_BIN", "curl"),
            docker: None,
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| default.into())
}

pub fn aws_bin() -> String {
    env_or("LTN_AWS_BIN", "aws")
}

pub fn tofu_bin() -> Option<String> {
    if let Some(p) = std::env::var("LTN_TOFU_BIN").ok().filter(|s| !s.trim().is_empty()) {
        return Some(p);
    }
    which("tofu").or_else(|| which("terraform")).map(|p| p.to_string_lossy().into_owned())
}
