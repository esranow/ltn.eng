//! Contracts shared by the runtime crates: the context every tool receives,
//! the tool definition the MCP server lists and dispatches, and the
//! provisioner interface that lets the engine create capacity it does not own.

use crate::docker::Docker;
use crate::store::{Store, Target};
use anyhow::{anyhow, Result};
use ltn_core::solver::placement::TargetCapacity;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct Ctx {
    /// Directory holding state.json and audit.jsonl.
    pub home: PathBuf,
    /// Docker CLI binary; overridable so tests can simulate a missing Docker.
    pub docker_bin: String,
    pub provisioners: Vec<Arc<dyn Provisioner>>,
}

impl Ctx {
    /// `LTN_HOME` (default `~/.ltn/runtime`) and `LTN_DOCKER_BIN` (default `docker`).
    pub fn from_env() -> Self {
        let home = std::env::var_os("LTN_HOME").map(PathBuf::from).unwrap_or_else(|| {
            let base = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(".ltn").join("runtime")
        });
        Self::for_home(&home)
    }

    pub fn for_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            docker_bin: std::env::var("LTN_DOCKER_BIN").unwrap_or_else(|_| "docker".into()),
            provisioners: Vec::new(),
        }
    }

    pub fn store(&self) -> Store {
        Store::new(&self.home)
    }

    /// Docker client for a target. `ssh://` hosts go through ltn's own ssh
    /// wrapper (ltn's ssh_config and known_hosts, never the user's).
    pub fn docker(&self, target: &Target) -> Docker {
        let d = Docker::new(&self.docker_bin, target.docker_host.as_deref());
        if !d.is_ssh() {
            return d;
        }
        match crate::ssh::ensure_ssh_shim(&self.home) {
            Ok(dir) => d.with_ssh_shim(dir),
            Err(e) => {
                eprintln!("ltn: ssh wrapper unavailable, using plain ssh: {e:#}");
                d
            }
        }
    }
}

pub type Handler = fn(&Ctx, &Value) -> Result<Value>;

/// One MCP tool. `input_schema` is a JSON Schema object.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub handler: Handler,
}

/// Capacity a provisioner can create on demand (e.g. one cloud instance type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Offer {
    /// Stable id, prefixed by provider, e.g. `aws:t3.small@us-east-1`.
    pub offer_id: String,
    pub provider: String,
    pub description: String,
    /// `target_id` must equal `offer_id`.
    pub capacity: TargetCapacity,
}

pub trait Provisioner: Send + Sync {
    fn provider(&self) -> &'static str;
    /// Offers currently known (from a cached catalog). Must not fail just
    /// because no credentials are configured: return an empty list instead.
    fn offers(&self, ctx: &Ctx) -> Result<Vec<Offer>>;
    /// Create the capacity and return a Docker-reachable target named `name`.
    fn provision(&self, ctx: &Ctx, offer_id: &str, name: &str) -> Result<Target>;
    fn deprovision(&self, ctx: &Ctx, target: &Target) -> Result<()>;
}

pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    arg_str_opt(args, key).ok_or_else(|| anyhow!("missing required string argument `{key}`"))
}

pub fn arg_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

pub fn arg_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}
