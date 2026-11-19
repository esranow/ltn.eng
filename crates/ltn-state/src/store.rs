//! The deployment record every tool reads and writes, plus an append-only
//! audit log. State is one JSON file replaced atomically on each save.

use crate::spec::AppSpec;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use ltn_core::solver::placement::{PlacementDecision, TargetCapacity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub id: String,
    /// None = the local Docker daemon; otherwise e.g. `ssh://deploy@10.0.0.5`.
    pub docker_host: Option<String>,
    /// Host used to reach published ports for health checks.
    pub address: String,
    pub cost_cents_per_hour: u64,
    /// GPU capacity for remote targets (local GPUs are probed with nvidia-smi).
    pub gpu_count: u64,
    pub vram_mb: u64,
    /// "local" | "ssh" | a cloud provisioner's name such as "aws".
    #[serde(default)]
    pub provider: String,
    /// Free-form data owned by whoever created the target (e.g. cloud instance id).
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl Target {
    pub fn local() -> Self {
        Self {
            id: "local".into(),
            docker_host: None,
            address: "127.0.0.1".into(),
            cost_cents_per_hour: 0,
            gpu_count: 0,
            vram_mb: 0,
            provider: "local".into(),
            labels: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub spec: AppSpec,
    /// An existing target's id, or a provisioner offer id when `provision_offer` is set.
    pub target_id: String,
    /// Set when the chosen capacity does not exist yet and apply must provision it first.
    #[serde(default)]
    pub provision_offer: Option<String>,
    pub decision: PlacementDecision,
    pub capacity_considered: Vec<TargetCapacity>,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub number: u32,
    pub image: String,
    pub plan_id: Option<String>,
    pub created_at: DateTime<Utc>,
    /// active | superseded | failed
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub app: String,
    pub target_id: String,
    pub spec: AppSpec,
    pub releases: Vec<Release>,
    pub current: Option<u32>,
    /// Consecutive failed health probes seen by the watcher.
    #[serde(default)]
    pub health_failures: u32,
}

impl Deployment {
    pub fn current_release(&self) -> Option<&Release> {
        self.current.and_then(|n| self.releases.iter().find(|r| r.number == n))
    }
    pub fn next_release_number(&self) -> u32 {
        self.releases.iter().map(|r| r.number).max().unwrap_or(0) + 1
    }
}

pub fn container_name(app: &str, release: u32) -> String {
    format!("ltn-{app}-r{release}")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub deployments: BTreeMap<String, Deployment>,
    #[serde(default)]
    pub plans: BTreeMap<String, Plan>,
    /// Per-crate extension data, keyed by crate (e.g. "cloud"). Lets a crate
    /// persist its own records without changing this struct.
    #[serde(default)]
    pub ext: BTreeMap<String, Value>,
}

impl State {
    /// Targets with the local daemon always present.
    pub fn all_targets(&self) -> Vec<Target> {
        let mut out = self.targets.clone();
        if !out.iter().any(|t| t.id == "local") {
            out.insert(0, Target::local());
        }
        out
    }
    pub fn target(&self, id: &str) -> Option<Target> {
        self.all_targets().into_iter().find(|t| t.id == id)
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }
    fn audit_path(&self) -> PathBuf {
        self.dir.join("audit.jsonl")
    }

    pub fn load(&self) -> Result<State> {
        match std::fs::read_to_string(self.state_path()) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("{} is corrupt", self.state_path().display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).context("reading ltn state"),
        }
    }

    /// Write to a temp file then rename, so a crash never leaves half a file.
    pub fn save(&self, state: &State) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("state.json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
        std::fs::rename(&tmp, self.state_path()).context("replacing ltn state file")?;
        Ok(())
    }

    /// Load, mutate and save under a lock (in-process mutex plus a lock file
    /// for other processes). Keep `f` short: never run docker or network calls
    /// inside it. Do slow work on a `load()` snapshot, then record the result here.
    pub fn update<T>(&self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        static IN_PROCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = IN_PROCESS.lock().unwrap_or_else(|p| p.into_inner());
        std::fs::create_dir_all(&self.dir)?;
        let _file_lock = FileLock::acquire(self.dir.join("state.lock"))?;
        let mut state = self.load()?;
        let out = f(&mut state)?;
        self.save(&state)?;
        Ok(out)
    }

    pub fn audit(&self, app: Option<&str>, action: &str, detail: Value) {
        let line = serde_json::json!({
            "at": Utc::now().to_rfc3339(),
            "app": app,
            "action": action,
            "detail": detail,
        });
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.audit_path()) {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn recent_audit(&self, app: Option<&str>, n: usize) -> Vec<Value> {
        let text = std::fs::read_to_string(self.audit_path()).unwrap_or_default();
        let mut out: Vec<Value> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| app.map_or(true, |a| v["app"].as_str() == Some(a)))
            .collect();
        let start = out.len().saturating_sub(n);
        out.drain(..start);
        out
    }
}

struct FileLock(PathBuf);

impl FileLock {
    fn acquire(path: PathBuf) -> Result<Self> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A holder that died leaves the file behind; break locks older than 30s.
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .map_or(false, |age| age.as_secs() > 30);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                    } else if std::time::Instant::now() > deadline {
                        anyhow::bail!("timed out waiting for {}", path.display());
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                }
                Err(e) => return Err(e).context("creating state lock file"),
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip_and_local_target_default() {
        let dir = std::env::temp_dir().join(format!("ltn-store-{}", uuid::Uuid::new_v4().simple()));
        let store = Store::new(&dir);
        let empty = store.load().unwrap();
        assert_eq!(empty.all_targets(), vec![Target::local()]);

        let mut st = State::default();
        st.targets.push(Target {
            id: "edge-1".into(),
            docker_host: Some("ssh://u@h".into()),
            address: "h".into(),
            cost_cents_per_hour: 5,
            provider: "ssh".into(),
            ..Target::local()
        });
        store.save(&st).unwrap();
        let back = store.load().unwrap();
        assert_eq!(back.all_targets().len(), 2);
        assert_eq!(back.target("edge-1").unwrap().cost_cents_per_hour, 5);
    }

    #[test]
    fn concurrent_updates_do_not_lose_writes() {
        let dir = std::env::temp_dir().join(format!("ltn-lock-{}", uuid::Uuid::new_v4().simple()));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    Store::new(&dir)
                        .update(|st| {
                            st.ext.insert(format!("k{i}"), serde_json::json!(i));
                            Ok(())
                        })
                        .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(Store::new(&dir).load().unwrap().ext.len(), 8);
    }

    #[test]
    fn audit_filters_by_app_and_keeps_last_n() {
        let dir = std::env::temp_dir().join(format!("ltn-audit-{}", uuid::Uuid::new_v4().simple()));
        let store = Store::new(&dir);
        for i in 0..5 {
            store.audit(Some("a"), "x", serde_json::json!({ "i": i }));
        }
        store.audit(Some("b"), "y", serde_json::json!({}));
        let a = store.recent_audit(Some("a"), 2);
        assert_eq!(a.len(), 2);
        assert_eq!(a[1]["detail"]["i"], 4);
        assert_eq!(store.recent_audit(None, 100).len(), 6);
    }
}
