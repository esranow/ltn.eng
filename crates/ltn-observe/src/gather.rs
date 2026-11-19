//! Everything that touches the real runtime: `docker inspect`, `docker stats`,
//! `docker logs`, `docker info` and the HTTP health probe.

use crate::model::{self, parse_stats, Probe, Stats, Verdict};
use anyhow::{anyhow, Result};
use ltn_state::docker::{ContainerState, Docker};
use ltn_state::store::{container_name, Deployment, State, Target};
use ltn_state::Ctx;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Timeout of one status/diagnose probe. Kept short so a status call over
/// many apps stays responsive; `LTN_PROBE_TIMEOUT_MS` overrides it.
pub fn probe_timeout() -> Duration {
    let ms = std::env::var("LTN_PROBE_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(5000);
    Duration::from_millis(ms)
}

/// One real GET. 2xx and 3xx are ok; redirects are not followed.
pub fn probe_http(url: &str, timeout: Duration) -> Probe {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .build();
    let start = Instant::now();
    let res = agent.get(url).call();
    let latency_ms = start.elapsed().as_millis() as u64;
    match res {
        Ok(resp) => {
            let s = resp.status();
            Probe {
                url: url.to_string(),
                ok: (200..400).contains(&s),
                status: Some(s),
                latency_ms,
                error: None,
                kind: if (200..400).contains(&s) { "ok".into() } else { "http_status".into() },
            }
        }
        Err(ureq::Error::Status(s, _)) => Probe {
            url: url.to_string(),
            ok: false,
            status: Some(s),
            latency_ms,
            error: None,
            kind: "http_status".into(),
        },
        Err(ureq::Error::Transport(t)) => {
            let msg = t.to_string();
            Probe {
                url: url.to_string(),
                ok: false,
                status: None,
                latency_ms,
                kind: Probe::classify_error(&msg).into(),
                error: Some(msg),
            }
        }
    }
}

/// Health URL of a deployment on its target, or None when the spec has no port.
pub fn health_url(dep: &Deployment, target: &Target) -> Option<String> {
    let port = dep.spec.host_port.or(dep.spec.port)?;
    Some(format!("http://{}:{}{}", target.address, port, dep.spec.health_path))
}

/// What the runtime says about one release of one deployment right now.
#[derive(Debug, Clone)]
pub struct Observation {
    pub app: String,
    pub release: Option<u32>,
    pub target_id: String,
    pub target: Option<Target>,
    pub container_name: Option<String>,
    pub target_error: Option<String>,
    pub inspect: Option<Value>,
    pub container: Option<ContainerState>,
    pub probe: Option<Probe>,
    /// Why no probe was made (no port, container not running, ...).
    pub probe_skipped: Option<String>,
    pub stats: Option<Stats>,
}

impl Observation {
    pub fn verdict(&self) -> (Verdict, Vec<String>) {
        if self.release.is_none() {
            return (Verdict::Missing, vec!["deployment has no current release".into()]);
        }
        model::verdict(
            self.target_error.as_deref(),
            self.container.as_ref(),
            self.probe.as_ref(),
            self.stats.as_ref(),
        )
    }

    pub fn has_port(&self, dep: &Deployment) -> bool {
        dep.spec.host_port.or(dep.spec.port).is_some()
    }

    pub fn docker(&self, ctx: &Ctx) -> Option<Docker> {
        self.target.as_ref().map(|t| ctx.docker(t))
    }

    /// The status JSON for one app (shape documented in the sprint plan).
    pub fn to_json(&self) -> Value {
        let (v, reasons) = self.verdict();
        let ports = self.inspect.as_ref().map(model::published_ports).unwrap_or_default();
        let uptime = self.container.as_ref().and_then(|c| model::uptime_s(c, chrono::Utc::now()));
        json!({
            "app": self.app,
            "verdict": v.as_str(),
            "reasons": reasons,
            "release": self.release,
            "target_id": self.target_id,
            "target_error": self.target_error,
            "container_name": self.container_name,
            "container": self.container.as_ref().map(|c| model::container_json(c, uptime, &ports)),
            "probe": match (&self.probe, &self.probe_skipped) {
                (Some(p), _) => serde_json::to_value(p).unwrap_or(Value::Null),
                (None, Some(why)) => json!({ "skipped": true, "reason": why }),
                (None, None) => Value::Null,
            },
            "stats": self.stats,
            "uptime_s": uptime,
        })
    }
}

pub fn find_target(state: &State, id: &str) -> Result<Target, String> {
    state
        .target(id)
        .ok_or_else(|| format!("target `{id}` is not registered (see ltn_targets)"))
}

/// Observes `release` (default: current) of `dep`. Never fails: whatever
/// could not be observed is recorded as an error in the observation.
pub fn observe(ctx: &Ctx, state: &State, dep: &Deployment, release: Option<u32>, with_stats: bool) -> Observation {
    let release = release.or(dep.current);
    let mut obs = Observation {
        app: dep.app.clone(),
        release,
        target_id: dep.target_id.clone(),
        target: None,
        container_name: release.map(|r| container_name(&dep.app, r)),
        target_error: None,
        inspect: None,
        container: None,
        probe: None,
        probe_skipped: None,
        stats: None,
    };
    let target = match find_target(state, &dep.target_id) {
        Ok(t) => t,
        Err(e) => {
            obs.target_error = Some(e);
            return obs;
        }
    };
    obs.target = Some(target.clone());
    let Some(name) = obs.container_name.clone() else {
        return obs;
    };
    let docker = ctx.docker(&target);
    match docker.inspect(&name) {
        Ok(Some(v)) => {
            obs.container = Some(ContainerState::from_inspect(&v));
            obs.inspect = Some(v);
        }
        Ok(None) => {}
        Err(e) => {
            // Inspect failing for a reason other than "no such container"
            // usually means the daemon is unreachable; ask `docker info` why.
            obs.target_error = Some(match docker.info() {
                Err(info_err) => info_err.to_string(),
                Ok(_) => e.to_string(),
            });
            return obs;
        }
    }
    let running = obs.container.as_ref().map_or(false, |c| c.running && !c.restarting);
    match health_url(dep, &target) {
        None => obs.probe_skipped = Some("spec declares no port; health = container running".into()),
        Some(_) if !running => obs.probe_skipped = Some("container is not running (stopped or restarting)".into()),
        Some(url) => obs.probe = Some(probe_http(&url, probe_timeout())),
    }
    if with_stats && running {
        if let Ok(Some(v)) = docker.stats(&name) {
            obs.stats = parse_stats(&v);
        }
    }
    obs
}

pub fn load_deployment(ctx: &Ctx, app: &str) -> Result<(State, Deployment)> {
    let state = ctx.store().load()?;
    let dep = state.deployments.get(app).cloned().ok_or_else(|| {
        let known: Vec<&str> = state.deployments.keys().map(String::as_str).collect();
        anyhow!(
            "no deployment named `{app}`. Known apps: {}. Deploy it with ltn_plan + ltn_apply.",
            if known.is_empty() { "none".to_string() } else { known.join(", ") }
        )
    })?;
    Ok((state, dep))
}

/// Most recent superseded release other than the current one.
pub fn previous_release(dep: &Deployment) -> Option<u32> {
    dep.releases
        .iter()
        .filter(|r| r.state == "superseded" && Some(r.number) != dep.current)
        .map(|r| r.number)
        .max()
}
