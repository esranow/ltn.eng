//! ltn_apply, rollback and ltn_destroy: the stop / run / health-gate /
//! restore procedure on a target's Docker daemon.

use crate::plan::check_plan_usable;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use ltn_state::docker::{tail_lines, ContainerState, Docker};
use ltn_state::spec::AppSpec;
use ltn_state::store::{container_name, Deployment, Release, State, Store, Target};
use ltn_state::tool::{arg_str, arg_str_opt, arg_u64_opt};
use ltn_state::Ctx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Images kept per app for rollback.
pub const KEEP_RELEASES: usize = 5;
pub const LOCKS_KEY: &str = "locks";
const LOCK_MINUTES: i64 = 10;
const STOP_TIMEOUT_S: &str = "10";

// ---------------------------------------------------------------- busy lock

/// True when `app` has an unexpired busy lock in `state`.
pub fn is_locked(state: &State, app: &str, now: DateTime<Utc>) -> bool {
    lock_expiry(state, app).map_or(false, |exp| exp > now)
}

fn lock_expiry(state: &State, app: &str) -> Option<DateTime<Utc>> {
    state
        .ext
        .get(LOCKS_KEY)
        .and_then(|l| l.get(app))
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// `State.ext["locks"][app]` = expiry while held; removed on drop.
#[derive(Debug)]
pub struct BusyLock {
    dir: PathBuf,
    app: String,
}

impl BusyLock {
    pub fn acquire(ctx: &Ctx, app: &str) -> Result<Self> {
        let now = Utc::now();
        ctx.store().update(|st| {
            if let Some(exp) = lock_expiry(st, app).filter(|e| *e > now) {
                bail!(
                    "`{app}` is busy: another apply/rollback/destroy holds its lock until {}. Wait and retry.",
                    exp.to_rfc3339()
                );
            }
            let locks = st.ext.entry(LOCKS_KEY.to_string()).or_insert_with(|| json!({}));
            if !locks.is_object() {
                *locks = json!({});
            }
            locks[app] = json!((now + Duration::minutes(LOCK_MINUTES)).to_rfc3339());
            Ok(())
        })?;
        Ok(Self { dir: ctx.home.clone(), app: app.to_string() })
    }
}

impl Drop for BusyLock {
    fn drop(&mut self) {
        let app = self.app.clone();
        let _ = Store::new(&self.dir).update(|st| {
            if let Some(l) = st.ext.get_mut(LOCKS_KEY).and_then(Value::as_object_mut) {
                l.remove(&app);
            }
            Ok(())
        });
    }
}

// ---------------------------------------------------------------- helpers

fn args_ref(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

pub fn image_tag(app: &str, release: u32) -> String {
    format!("ltn/{app}:r{release}")
}

/// `docker run` arguments per the conventions (name, labels, restart policy, limits, port, env).
pub fn run_args(spec: &AppSpec, app: &str, release: u32, image: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        container_name(app, release),
        "--label".into(),
        format!("ltn.app={app}"),
        "--label".into(),
        format!("ltn.release={release}"),
        "--restart".into(),
        "unless-stopped".into(),
        "--cpus".into(),
        format!("{:.3}", spec.cpu_millis as f64 / 1000.0),
        "--memory".into(),
        format!("{}m", spec.ram_mb),
    ];
    if let (Some(port), Some(hp)) = (spec.port, spec.host_port) {
        a.push("-p".into());
        a.push(format!("{hp}:{port}"));
    }
    if spec.gpu_count > 0 {
        a.push("--gpus".into());
        a.push(spec.gpu_count.to_string());
    }
    for (k, v) in &spec.env {
        a.push("-e".into());
        a.push(format!("{k}={v}"));
    }
    a.push(image.to_string());
    a
}

pub fn endpoint(spec: &AppSpec, target: &Target) -> Option<String> {
    match (spec.port, spec.host_port) {
        (Some(_), Some(hp)) => Some(format!("http://{}:{hp}", target.address)),
        _ => None,
    }
}

fn health_url(spec: &AppSpec, target: &Target) -> Option<String> {
    endpoint(spec, target).map(|e| format!("{e}{}", spec.health_path))
}

/// GET `url`; Ok(status) on 2xx/3xx.
pub fn http_probe(url: &str) -> std::result::Result<u16, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(3))
        .redirects(0)
        .build();
    match agent.get(url).call() {
        Ok(r) if (200..400).contains(&r.status()) => Ok(r.status()),
        Ok(r) => Err(format!("HTTP {}", r.status())),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(e) => Err(e.to_string()),
    }
}

/// Healthy = running, not restarting, RestartCount 0, and (with a port) the
/// health URL answering 2xx/3xx, all before `timeout_s`. Fails fast when the
/// container exits or restarts.
fn health_gate(docker: &Docker, name: &str, url: Option<&str>, timeout_s: u64) -> std::result::Result<Value, String> {
    let start = Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_s.max(1));
    // Without a port the only signal is the container staying up.
    let settle = std::time::Duration::from_secs(timeout_s.clamp(1, 5));
    let mut last_probe = String::from("no probe yet");
    loop {
        let v = docker
            .inspect(name)
            .map_err(|e| format!("could not inspect the new container: {e}"))?
            .ok_or_else(|| "the new container disappeared".to_string())?;
        let cs = ContainerState::from_inspect(&v);
        if cs.restart_count > 0 || cs.restarting {
            // While running again after a restart, ExitCode is reset to 0, so
            // only quote it when Docker is between restarts.
            return Err(format!(
                "container crashed and was restarted by Docker (restart_count={}{}{})",
                cs.restart_count,
                if cs.restarting { format!(", last exit code {}", cs.exit_code) } else { String::new() },
                if cs.oom_killed { ", OOMKilled" } else { "" }
            ));
        }
        if !cs.running {
            return Err(format!(
                "container exited with code {}{} (status {})",
                cs.exit_code,
                if cs.oom_killed { ", OOMKilled" } else { "" },
                cs.status
            ));
        }
        match url {
            Some(u) => match http_probe(u) {
                Ok(code) => {
                    // Confirm it did not restart while we were probing.
                    let again = docker.inspect(name).ok().flatten().map(|v| ContainerState::from_inspect(&v));
                    if let Some(cs) = again.filter(|c| c.running && !c.restarting && c.restart_count == 0) {
                        return Ok(json!({
                            "healthy": true, "url": u, "status_code": code,
                            "elapsed_ms": start.elapsed().as_millis() as u64,
                            "container": cs,
                        }));
                    }
                    last_probe = format!("HTTP {code} but the container restarted");
                }
                Err(e) => last_probe = e,
            },
            None => {
                if start.elapsed() >= settle {
                    return Ok(json!({
                        "healthy": true, "url": null,
                        "check": format!("running with no restarts for {}s (no port, so no HTTP probe)", settle.as_secs()),
                        "elapsed_ms": start.elapsed().as_millis() as u64,
                        "container": cs,
                    }));
                }
            }
        }
        if start.elapsed() >= timeout {
            return Err(format!(
                "health check timed out after {timeout_s}s; last probe of {}: {last_probe}",
                url.unwrap_or("(none)")
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Inspect state and logs of a failed container, after turning its restart
/// policy off and giving a crash-looping container a moment to settle so the
/// real exit code is visible.
fn capture_failure(docker: &Docker, name: &str, crashed: bool) -> Value {
    let mut updated = docker.run(&["update", "--restart", "no", name]).map_or(false, |r| r.ok);
    let mut state = None;
    // A crash-looping container is given a few seconds to exit for good once
    // its restart policy is off; a hung one is captured as it is.
    let deadline = Instant::now() + std::time::Duration::from_secs(if crashed { 8 } else { 0 });
    loop {
        if !updated {
            // Docker refuses updates while a container is restarting; retry.
            updated = docker.run(&["update", "--restart", "no", name]).map_or(false, |r| r.ok);
        }
        let cs = docker.inspect(name).ok().flatten().map(|v| ContainerState::from_inspect(&v));
        let settled = cs.as_ref().map_or(true, |c| !c.running && !c.restarting);
        state = cs.or(state);
        if settled || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let logs = docker.logs(name, 50).unwrap_or_else(|e| format!("(logs unavailable: {e})"));
    json!({ "container": name, "state": state, "logs": logs })
}

fn wait_running(docker: &Docker, name: &str, secs: u64) -> bool {
    let deadline = Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        let running = docker
            .inspect(name)
            .ok()
            .flatten()
            .map(|v| ContainerState::from_inspect(&v))
            .map_or(false, |c| c.running && !c.restarting);
        if running {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

// ---------------------------------------------------------------- swap

struct SwapOk {
    health: Value,
    container: String,
}

struct SwapFail {
    reason: String,
    evidence: Value,
    /// None = there was no running previous release to restore.
    restored: Option<bool>,
}

impl SwapFail {
    fn message(&self, what: &str) -> String {
        let st = &self.evidence["state"];
        let restored = match self.restored {
            Some(true) => "the previous release was restarted and is running again".to_string(),
            Some(false) => "the previous release could NOT be restarted; the app is down".to_string(),
            None => "there was no running previous release to restore".to_string(),
        };
        format!(
            "{what} failed: {}.\nNew container state: status={} exit_code={} oom_killed={} restart_count={} error={}\n\
             Last 50 log lines:\n{}\nPrevious release: {restored}.\nThe new container was removed. Fix the cause \
             shown above, then plan and apply again.",
            self.reason,
            st["status"].as_str().unwrap_or("unknown"),
            st["exit_code"],
            st["oom_killed"],
            st["restart_count"],
            st["error"].as_str().unwrap_or(""),
            self.evidence["logs"].as_str().filter(|l| !l.trim().is_empty()).unwrap_or("(the container printed nothing)"),
        )
    }
}

/// Old container to replace: (docker of its target, name, same target as new?).
struct OldContainer {
    docker: Docker,
    name: String,
    same_target: bool,
}

/// Stop the old container (same target only), run the new one, health-gate
/// it. On success remove the old container; on failure remove the new one
/// and restart the old one.
fn swap(
    docker: &Docker,
    target: &Target,
    spec: &AppSpec,
    app: &str,
    release: u32,
    image: &str,
    old: Option<&OldContainer>,
) -> std::result::Result<SwapOk, SwapFail> {
    let new_name = container_name(app, release);
    let mut stopped_old: Option<&OldContainer> = None;
    if let Some(o) = old.filter(|o| o.same_target && o.name != new_name) {
        let running = match o.docker.inspect(&o.name) {
            Ok(v) => v.map_or(false, |v| ContainerState::from_inspect(&v).running),
            Err(e) => {
                return Err(SwapFail {
                    reason: format!("could not inspect the current container {}: {e}", o.name),
                    evidence: json!({}),
                    restored: None,
                })
            }
        };
        if running {
            if let Err(e) = o.docker.run_ok(&["stop", "-t", STOP_TIMEOUT_S, &o.name]) {
                return Err(SwapFail {
                    reason: format!("could not stop the current container {}: {e}", o.name),
                    evidence: json!({}),
                    restored: None,
                });
            }
            stopped_old = Some(o);
        }
    }

    let restore = |reason: String, evidence: Value| -> SwapFail {
        let _ = docker.run(&["rm", "-f", &new_name]);
        let restored = stopped_old.map(|o| o.docker.run_ok(&["start", &o.name]).is_ok() && wait_running(&o.docker, &o.name, 15));
        SwapFail { reason, evidence, restored }
    };

    let _ = docker.run(&["rm", "-f", &new_name]);
    let args = run_args(spec, app, release, image);
    match docker.run(&args_ref(&args)) {
        Ok(r) if r.ok => {}
        Ok(r) => {
            let evidence = json!({ "container": new_name, "state": null, "logs": tail_lines(&r.stderr, 50) });
            return Err(restore(format!("`docker run` failed: {}", tail_lines(&r.stderr, 10)), evidence));
        }
        Err(e) => return Err(restore(format!("`docker run` could not start: {e}"), json!({ "container": new_name }))),
    }

    let url = health_url(spec, target);
    match health_gate(docker, &new_name, url.as_deref(), spec.health_timeout_s) {
        Ok(health) => {
            if let Some(o) = old.filter(|o| o.name != new_name) {
                let _ = o.docker.run(&["rm", "-f", &o.name]);
            }
            Ok(SwapOk { health, container: new_name })
        }
        Err(mut reason) => {
            let crashed = reason.starts_with("container ");
            let evidence = capture_failure(docker, &new_name, crashed);
            let st = &evidence["state"];
            if crashed && st["status"] == "exited" && !reason.contains("exited with code") {
                reason = format!("{reason}; with restarts off it exits with code {}", st["exit_code"]);
            }
            Err(restore(reason, evidence))
        }
    }
}

fn old_container(ctx: &Ctx, state: &State, d: Option<&Deployment>, new_target: &Target) -> Option<OldContainer> {
    let d = d?;
    let r = d.current_release()?;
    let t = state.target(&d.target_id)?;
    Some(OldContainer { docker: ctx.docker(&t), name: container_name(&d.app, r.number), same_target: t.id == new_target.id })
}

// ---------------------------------------------------------------- apply

fn provision(ctx: &Ctx, offer_id: &str, app: &str) -> Result<Target> {
    if std::env::var("LTN_ALLOW_CLOUD_SPEND").ok().as_deref() != Some("1") {
        bail!(
            "plan needs new capacity from offer `{offer_id}`, which creates billable cloud resources. \
             Refused: set LTN_ALLOW_CLOUD_SPEND=1 in the server environment to allow it."
        );
    }
    let p = ctx
        .provisioners
        .iter()
        .find(|p| offer_id.starts_with(&format!("{}:", p.provider())))
        .or_else(|| {
            ctx.provisioners
                .iter()
                .find(|p| p.offers(ctx).map_or(false, |l| l.iter().any(|o| o.offer_id == offer_id)))
        })
        .ok_or_else(|| anyhow!("no provisioner is configured for offer `{offer_id}`"))?;
    let name = format!("{app}-{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    let target = p.provision(ctx, offer_id, &name).with_context(|| format!("provisioning `{offer_id}` failed"))?;
    let store = ctx.store();
    store.update(|st| {
        st.targets.retain(|t| t.id != target.id);
        st.targets.push(target.clone());
        Ok(())
    })?;
    store.audit(Some(app), "provision", json!({ "offer_id": offer_id, "target": target }));
    Ok(target)
}

pub fn apply(ctx: &Ctx, args: &Value) -> Result<Value> {
    let plan_id = arg_str(args, "plan_id")?;
    let store = ctx.store();
    let plan = store
        .load()?
        .plans
        .get(plan_id)
        .cloned()
        .ok_or_else(|| anyhow!("no plan `{plan_id}`. ltn_apply only accepts a plan_id returned by ltn_plan."))?;
    check_plan_usable(&plan, Utc::now())?;
    let spec = plan.spec.clone();
    let app = spec.name.clone();
    let _lock = BusyLock::acquire(ctx, &app)?;

    let state = store.load()?;
    if state.plans.get(plan_id).map_or(true, |p| p.applied) {
        bail!("plan `{plan_id}` was applied concurrently");
    }
    let target = match &plan.provision_offer {
        Some(offer) => provision(ctx, offer, &app)?,
        None => state
            .target(&plan.target_id)
            .ok_or_else(|| anyhow!("target `{}` from the plan no longer exists; plan again", plan.target_id))?,
    };
    let existing = state.deployments.get(&app).cloned();
    let release = existing.as_ref().map_or(1, Deployment::next_release_number);
    let image = image_tag(&app, release);
    let docker = ctx.docker(&target);

    // Build.
    let dockerfile = Path::new(&spec.path).join(&spec.dockerfile);
    let dockerfile = dockerfile.to_string_lossy().to_string();
    let r = docker
        .run(&["build", "-t", &image, "-f", &dockerfile, &spec.path])
        .with_context(|| format!("building {image} on target `{}`", target.id))?;
    if !r.ok {
        let output = format!("{}\n{}", r.stdout, r.stderr);
        record_failure(&store, existing.is_some(), &app, release, &image, Some(plan_id), json!({ "stage": "build" }));
        bail!(
            "`docker build` of {image} on target `{}` failed (exit {:?}). Nothing was deployed; the current \
             release is untouched. Build output tail:\n{}",
            target.id,
            r.code,
            tail_lines(output.trim(), 40)
        );
    }

    let old = old_container(ctx, &state, existing.as_ref(), &target);
    let from_release = existing.as_ref().and_then(|d| d.current);
    match swap(&docker, &target, &spec, &app, release, &image, old.as_ref()) {
        Ok(ok) => {
            // Keep the newest KEEP_RELEASES images (including this one) for rollback.
            let mut numbers: Vec<u32> = existing.as_ref().map(|d| d.releases.iter().map(|r| r.number).collect()).unwrap_or_default();
            numbers.push(release);
            numbers.sort_unstable_by(|a, b| b.cmp(a));
            let pruned: Vec<String> = numbers
                .iter()
                .skip(KEEP_RELEASES)
                .map(|n| image_tag(&app, *n))
                .filter(|img| docker.run(&["rmi", img]).map_or(false, |r| r.ok))
                .collect();
            let now = Utc::now();
            store.update(|st| {
                let d = st.deployments.entry(app.clone()).or_insert_with(|| Deployment {
                    app: app.clone(),
                    target_id: target.id.clone(),
                    spec: spec.clone(),
                    releases: Vec::new(),
                    current: None,
                    health_failures: 0,
                });
                if let Some(cur) = d.current {
                    if let Some(r) = d.releases.iter_mut().find(|r| r.number == cur) {
                        r.state = "superseded".into();
                    }
                }
                d.releases.push(Release {
                    number: release,
                    image: image.clone(),
                    plan_id: Some(plan_id.to_string()),
                    created_at: now,
                    state: "active".into(),
                });
                d.current = Some(release);
                d.spec = spec.clone();
                d.target_id = target.id.clone();
                d.health_failures = 0;
                if let Some(p) = st.plans.get_mut(plan_id) {
                    p.applied = true;
                }
                Ok(())
            })?;
            let out = json!({
                "app": app,
                "release": release,
                "from_release": from_release,
                "target_id": target.id,
                "container": ok.container,
                "image": image,
                "endpoint": endpoint(&spec, &target),
                "health": ok.health,
                "provisioned": plan.provision_offer.is_some(),
                "pruned_images": pruned,
            });
            store.audit(Some(&app), "apply", out.clone());
            Ok(out)
        }
        Err(fail) => {
            let _ = docker.run(&["rmi", &image]);
            record_failure(
                &store,
                existing.is_some(),
                &app,
                release,
                &image,
                Some(plan_id),
                json!({ "stage": "health", "reason": fail.reason, "evidence": fail.evidence, "restored": fail.restored }),
            );
            bail!(fail.message(&format!("apply of {app} r{release} on target `{}`", target.id)))
        }
    }
}

/// Record a failed release on an existing deployment (a first deploy that
/// fails leaves no deployment record) and audit it.
fn record_failure(store: &Store, has_deployment: bool, app: &str, release: u32, image: &str, plan_id: Option<&str>, detail: Value) {
    if has_deployment {
        let _ = store.update(|st| {
            if let Some(d) = st.deployments.get_mut(app) {
                if !d.releases.iter().any(|r| r.number == release) {
                    d.releases.push(Release {
                        number: release,
                        image: image.to_string(),
                        plan_id: plan_id.map(str::to_string),
                        created_at: Utc::now(),
                        state: "failed".into(),
                    });
                }
            }
            Ok(())
        });
    }
    let mut detail = detail;
    detail["release"] = json!(release);
    store.audit(Some(app), "apply_failed", detail);
}

// ---------------------------------------------------------------- rollback

/// Release to roll back to: `to`, or the newest "superseded" release older than current.
pub fn rollback_target(d: &Deployment, to: Option<u32>) -> Result<u32> {
    match to {
        Some(n) => {
            let r = d
                .releases
                .iter()
                .find(|r| r.number == n)
                .ok_or_else(|| anyhow!("`{}` has no release {n}", d.app))?;
            if d.current == Some(n) {
                bail!("release {n} of `{}` is already current", d.app);
            }
            if r.state == "failed" {
                bail!("release {n} of `{}` failed its health gate before; pick another release", d.app);
            }
            Ok(n)
        }
        None => d
            .releases
            .iter()
            .filter(|r| r.state == "superseded" && d.current.map_or(true, |c| r.number < c))
            .map(|r| r.number)
            .max()
            .ok_or_else(|| anyhow!("`{}` has no earlier superseded release to roll back to", d.app)),
    }
}

pub fn rollback(ctx: &Ctx, app: &str, to_release: Option<u32>) -> Result<Value> {
    let store = ctx.store();
    let state = store.load()?;
    let d = state
        .deployments
        .get(app)
        .cloned()
        .ok_or_else(|| anyhow!("no deployment named `{app}`"))?;
    let to = rollback_target(&d, to_release)?;
    let _lock = BusyLock::acquire(ctx, app)?;
    let state = store.load()?;
    let d = state.deployments.get(app).cloned().ok_or_else(|| anyhow!("`{app}` was destroyed concurrently"))?;
    let target = state
        .target(&d.target_id)
        .ok_or_else(|| anyhow!("target `{}` of `{app}` no longer exists", d.target_id))?;
    let docker = ctx.docker(&target);
    let rel = d.releases.iter().find(|r| r.number == to).cloned().ok_or_else(|| anyhow!("release {to} vanished"))?;
    docker.info().with_context(|| format!("target `{}` is not reachable", target.id))?;
    if !docker.image_exists(&rel.image) {
        bail!(
            "cannot roll `{app}` back to release {to}: image {} no longer exists on target `{}` (only the newest \
             {KEEP_RELEASES} release images are kept). Deploy a fixed version with ltn_plan + ltn_apply instead.",
            rel.image,
            target.id
        );
    }
    let old = old_container(ctx, &state, Some(&d), &target);
    let from_release = d.current;
    match swap(&docker, &target, &d.spec, app, to, &rel.image, old.as_ref()) {
        Ok(ok) => {
            store.update(|st| {
                if let Some(d) = st.deployments.get_mut(app) {
                    for r in d.releases.iter_mut() {
                        if Some(r.number) == from_release {
                            r.state = "rolled_back".into();
                        } else if r.number == to {
                            r.state = "active".into();
                        }
                    }
                    d.current = Some(to);
                    d.health_failures = 0;
                }
                Ok(())
            })?;
            let out = json!({
                "app": app,
                "release": to,
                "from_release": from_release,
                "target_id": target.id,
                "container": ok.container,
                "image": rel.image,
                "endpoint": endpoint(&d.spec, &target),
                "health": ok.health,
            });
            store.audit(Some(app), "rollback", out.clone());
            Ok(out)
        }
        Err(fail) => {
            let _ = store.update(|st| {
                if let Some(r) = st.deployments.get_mut(app).and_then(|d| d.releases.iter_mut().find(|r| r.number == to)) {
                    r.state = "failed".into();
                }
                Ok(())
            });
            store.audit(
                Some(app),
                "rollback_failed",
                json!({ "release": to, "from_release": from_release, "reason": fail.reason, "evidence": fail.evidence, "restored": fail.restored }),
            );
            bail!(fail.message(&format!("rollback of {app} to r{to} on target `{}`", target.id)))
        }
    }
}

pub fn rollback_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = arg_str(args, "app")?;
    let to = arg_u64_opt(args, "to_release").map(|n| u32::try_from(n)).transpose().map_err(|_| anyhow!("to_release is too large"))?;
    rollback(ctx, app, to)
}

// ---------------------------------------------------------------- destroy

pub fn destroy(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = arg_str(args, "app")?;
    if arg_str_opt(args, "confirm") != Some(app) {
        bail!("refusing to destroy `{app}`: pass confirm = \"{app}\" (the app name) to confirm.");
    }
    let store = ctx.store();
    let state = store.load()?;
    let deployment = state.deployments.get(app).cloned();
    let target = match &deployment {
        Some(d) => state
            .target(&d.target_id)
            .ok_or_else(|| anyhow!("target `{}` of `{app}` no longer exists", d.target_id))?,
        None => Target::local(),
    };
    let _lock = BusyLock::acquire(ctx, app)?;
    let docker = ctx.docker(&target);
    let containers = docker.containers_with_label(&format!("ltn.app={app}"))?;
    for c in &containers {
        docker.run_ok(&["rm", "-f", c]).with_context(|| format!("removing container {c}"))?;
    }
    let repo = format!("ltn/{app}");
    let images: Vec<String> = docker
        .run_ok(&["images", &repo, "--format", "{{.Repository}}:{{.Tag}}"])?
        .lines()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect();
    let mut removed_images = Vec::new();
    for img in &images {
        if docker.run(&["rmi", "-f", img]).map_or(false, |r| r.ok) {
            removed_images.push(img.clone());
        }
    }
    if deployment.is_none() && containers.is_empty() && images.is_empty() {
        bail!("no deployment, containers or images found for `{app}`");
    }
    store.update(|st| {
        st.deployments.remove(app);
        st.plans.retain(|_, p| p.spec.name != app);
        Ok(())
    })?;
    let out = json!({
        "app": app,
        "target_id": target.id,
        "removed_containers": containers,
        "removed_images": removed_images,
    });
    store.audit(Some(app), "destroy", out.clone());
    Ok(out)
}
