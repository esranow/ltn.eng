//! The watch loop: one `watch_tick` per interval checks every deployment,
//! starts what is stopped, restarts what keeps failing its probe, and calls
//! the rollback hook once per release when repairs did not help.
//!
//! Persistent memory lives in `State.ext["observe"]["apps"][<app>]`:
//! `{ release, repairs, rollback_attempted, condition }`.

use crate::gather::{self, Observation};
use crate::RollbackHook;
use ltn_state::store::State;
use ltn_state::Ctx;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Consecutive failed probes before the watcher restarts a running container.
pub const PROBE_FAILURES_BEFORE_RESTART: u32 = 3;
/// Repair attempts (start/restart) on one release before rolling back.
pub const REPAIRS_BEFORE_ROLLBACK: u32 = 2;
/// Docker RestartCount at which a release counts as crash-looping.
pub const RESTART_COUNT_ROLLBACK: i64 = 3;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WatchEvent {
    pub app: String,
    pub release: Option<u32>,
    /// healthy | skipped_locked | started | start_failed | probe_failed |
    /// restarted | restart_failed | restarting | rollback | rollback_failed |
    /// container_missing | target_unreachable | gave_up | error
    pub kind: String,
    pub detail: Value,
}

/// What the watcher remembers about one app between ticks.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AppMemory {
    pub release: u32,
    pub repairs: u32,
    pub rollback_attempted: bool,
    /// Last reported condition, so missing/unreachable is audited once per change.
    pub condition: String,
}

/// What the runtime showed this tick, reduced to what the decision needs.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    TargetUnreachable(String),
    Missing,
    /// Exists, not running, not restarting.
    Stopped { exit_code: i64, restart_count: i64 },
    /// Docker's restart policy is cycling it.
    Restarting { restart_count: i64 },
    /// Running; `probe_ok` is None when the spec has no port.
    Running { probe_ok: Option<bool>, restart_count: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Healthy: reset probe failures and repair count.
    Healthy,
    Start,
    /// Probe failed; new consecutive failure count.
    CountFailure(u32),
    Restart,
    Rollback,
    /// Report a condition (audited only when it changed).
    Report(&'static str),
    /// Rollback already attempted for this release: stop acting on it.
    GaveUp,
}

/// Pure decision for one app this tick.
pub fn decide(seen: &Seen, health_failures: u32, mem: &AppMemory) -> Decision {
    match seen {
        Seen::TargetUnreachable(_) => return Decision::Report("target_unreachable"),
        Seen::Missing => return Decision::Report("container_missing"),
        _ => {}
    }
    let restart_count = match seen {
        Seen::Stopped { restart_count, .. } | Seen::Restarting { restart_count } | Seen::Running { restart_count, .. } => *restart_count,
        _ => 0,
    };
    let healthy = matches!(seen, Seen::Running { probe_ok: Some(true) | None, .. });
    if mem.rollback_attempted {
        return if healthy { Decision::Healthy } else { Decision::GaveUp };
    }
    if restart_count >= RESTART_COUNT_ROLLBACK {
        return Decision::Rollback;
    }
    if healthy {
        return Decision::Healthy;
    }
    if mem.repairs >= REPAIRS_BEFORE_ROLLBACK {
        return Decision::Rollback;
    }
    match seen {
        Seen::Stopped { .. } => Decision::Start,
        Seen::Restarting { .. } => Decision::CountFailure(health_failures + 1),
        Seen::Running { .. } => {
            let n = health_failures + 1;
            if n >= PROBE_FAILURES_BEFORE_RESTART {
                Decision::Restart
            } else {
                Decision::CountFailure(n)
            }
        }
        _ => unreachable!(),
    }
}

pub fn seen_from(obs: &Observation) -> Seen {
    if let Some(e) = &obs.target_error {
        return Seen::TargetUnreachable(e.clone());
    }
    let Some(c) = &obs.container else {
        return Seen::Missing;
    };
    if c.restarting {
        Seen::Restarting { restart_count: c.restart_count }
    } else if !c.running {
        Seen::Stopped { exit_code: c.exit_code, restart_count: c.restart_count }
    } else {
        Seen::Running { probe_ok: obs.probe.as_ref().map(|p| p.ok), restart_count: c.restart_count }
    }
}

/// True when `State.ext["locks"][app]` holds an RFC 3339 time in the future.
pub fn is_locked(state: &State, app: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    state
        .ext
        .get("locks")
        .and_then(|l| l.get(app))
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or(false, |t| t.with_timezone(&chrono::Utc) > now)
}

/// The same per-app busy lock the engine takes for apply/rollback/destroy
/// (`State.ext["locks"][app]` = expiry), taken atomically and removed on drop.
/// Restart/start by the watcher or `ltn_repair` holds it, so they can never act
/// on a container that an apply is replacing at the same moment.
#[derive(Debug)]
pub struct AppLock {
    home: std::path::PathBuf,
    app: String,
}

impl AppLock {
    pub fn acquire(ctx: &Ctx, app: &str, holder: &str) -> anyhow::Result<Self> {
        let now = chrono::Utc::now();
        ctx.store().update(|st| {
            if is_locked(st, app, now) {
                anyhow::bail!(
                    "`{app}` is busy: an apply/rollback/destroy/repair holds its lock until {}. Wait and retry.",
                    st.ext["locks"][app].as_str().unwrap_or("?")
                );
            }
            let locks = st.ext.entry("locks".to_string()).or_insert_with(|| json!({}));
            if !locks.is_object() {
                *locks = json!({});
            }
            locks[app] = json!((now + chrono::Duration::minutes(10)).to_rfc3339());
            Ok(())
        })?;
        let _ = holder; // who asked; reserved for audit
        Ok(Self { home: ctx.home.clone(), app: app.to_string() })
    }
}

impl Drop for AppLock {
    fn drop(&mut self) {
        let app = self.app.clone();
        let _ = ltn_state::store::Store::new(&self.home).update(|st| {
            if let Some(l) = st.ext.get_mut("locks").and_then(Value::as_object_mut) {
                l.remove(&app);
            }
            Ok(())
        });
    }
}

fn memory_of(state: &State, app: &str, release: u32) -> AppMemory {
    let m: Option<AppMemory> = state
        .ext
        .get("observe")
        .and_then(|o| o.get("apps"))
        .and_then(|a| a.get(app))
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    match m {
        Some(m) if m.release == release => m,
        // A new release starts with a clean slate (but keeps the condition).
        Some(m) => AppMemory { release, condition: m.condition, ..Default::default() },
        None => AppMemory { release, ..Default::default() },
    }
}

fn save_memory(state: &mut State, app: &str, mem: &AppMemory) {
    let root = state.ext.entry("observe".to_string()).or_insert_with(|| json!({}));
    if !root.is_object() {
        *root = json!({});
    }
    if !root["apps"].is_object() {
        root["apps"] = json!({});
    }
    root["apps"][app] = serde_json::to_value(mem).unwrap_or(Value::Null);
}

/// One pass over every deployment with a current release.
pub fn watch_tick(ctx: &Ctx, rollback: RollbackHook) -> Vec<WatchEvent> {
    let store = ctx.store();
    let mut events = Vec::new();
    let snapshot = match store.load() {
        Ok(s) => s,
        Err(e) => {
            events.push(WatchEvent { app: String::new(), release: None, kind: "error".into(), detail: json!({ "error": e.to_string() }) });
            return events;
        }
    };
    for dep in snapshot.deployments.values() {
        let Some(release) = dep.current else { continue };
        let app = dep.app.as_str();
        let ev = |kind: &str, detail: Value| WatchEvent { app: app.to_string(), release: Some(release), kind: kind.into(), detail };
        // Re-read for the lock: the engine may have taken it since the snapshot.
        let fresh = store.load().unwrap_or_else(|_| snapshot.clone());
        if is_locked(&fresh, app, chrono::Utc::now()) {
            events.push(ev("skipped_locked", json!({ "lock": fresh.ext["locks"][app] })));
            continue;
        }
        let obs = gather::observe(ctx, &fresh, dep, Some(release), false);
        let seen = seen_from(&obs);
        let mut mem = memory_of(&fresh, app, release);
        let health_failures = fresh.deployments.get(app).map_or(0, |d| d.health_failures);
        let decision = decide(&seen, health_failures, &mem);
        let container = obs.container_name.clone().unwrap_or_default();
        let docker = obs.docker(ctx);

        let condition = match &decision {
            Decision::Report(c) => c.to_string(),
            Decision::Healthy => "healthy".into(),
            Decision::GaveUp => "gave_up".into(),
            _ => "unhealthy".into(),
        };
        let condition_changed = condition != mem.condition;
        mem.condition = condition;

        let mut reset_failures = false;
        let mut set_failures: Option<u32> = None;
        match decision {
            Decision::Report(kind) => {
                let detail = match &seen {
                    Seen::TargetUnreachable(e) => json!({ "target_id": dep.target_id, "error": e }),
                    _ => json!({ "container": container, "target_id": dep.target_id }),
                };
                if condition_changed {
                    store.audit(Some(app), &format!("watch.{kind}"), detail.clone());
                }
                events.push(ev(kind, json!({ "detail": detail, "first_seen": condition_changed })));
            }
            Decision::Healthy => {
                if health_failures > 0 || mem.repairs > 0 {
                    if mem.repairs > 0 {
                        store.audit(Some(app), "watch.recovered", json!({ "container": container, "repairs": mem.repairs }));
                    }
                    reset_failures = true;
                    mem.repairs = 0;
                }
                events.push(ev("healthy", json!({ "container": container })));
            }
            Decision::Start => {
                mem.repairs += 1;
                let res = match AppLock::acquire(ctx, app, "watcher") {
                    Ok(_lock) => docker.as_ref().map(|d| d.run_ok(&["start", &container])),
                    Err(e) => Some(Err(e)),
                };
                match res {
                    Some(Ok(_)) => {
                        let detail = json!({ "container": container, "attempt": mem.repairs, "was": format!("{seen:?}") });
                        store.audit(Some(app), "watch.start", detail.clone());
                        events.push(ev("started", detail));
                    }
                    Some(Err(e)) => {
                        let detail = json!({ "container": container, "attempt": mem.repairs, "error": e.to_string() });
                        store.audit(Some(app), "watch.start_failed", detail.clone());
                        events.push(ev("start_failed", detail));
                    }
                    None => events.push(ev("error", json!({ "error": "target not found" }))),
                }
            }
            Decision::CountFailure(n) => {
                set_failures = Some(n);
                let probe = obs.probe.as_ref().map(|p| serde_json::to_value(p).unwrap_or(Value::Null));
                let kind = if matches!(seen, Seen::Restarting { .. }) { "restarting" } else { "probe_failed" };
                events.push(ev(kind, json!({ "consecutive_failures": n, "probe": probe })));
            }
            Decision::Restart => {
                mem.repairs += 1;
                reset_failures = true;
                let res = match AppLock::acquire(ctx, app, "watcher") {
                    Ok(_lock) => docker.as_ref().map(|d| d.run_ok(&["restart", &container])),
                    Err(e) => Some(Err(e)),
                };
                let probe = obs.probe.as_ref().map(|p| serde_json::to_value(p).unwrap_or(Value::Null));
                match res {
                    Some(Ok(_)) => {
                        let detail = json!({ "container": container, "attempt": mem.repairs, "failed_probes": health_failures + 1, "last_probe": probe });
                        store.audit(Some(app), "watch.restart", detail.clone());
                        events.push(ev("restarted", detail));
                    }
                    Some(Err(e)) => {
                        let detail = json!({ "container": container, "attempt": mem.repairs, "error": e.to_string() });
                        store.audit(Some(app), "watch.restart_failed", detail.clone());
                        events.push(ev("restart_failed", detail));
                    }
                    None => events.push(ev("error", json!({ "error": "target not found" }))),
                }
            }
            Decision::Rollback => {
                // Record the attempt before calling, so a crash never loops on it.
                mem.rollback_attempted = true;
                let mem_now = mem.clone();
                let _ = store.update(|st| {
                    save_memory(st, app, &mem_now);
                    Ok(())
                });
                let why = json!({
                    "seen": format!("{seen:?}"),
                    "repairs": mem.repairs,
                    "restart_count_threshold": RESTART_COUNT_ROLLBACK,
                    "repairs_threshold": REPAIRS_BEFORE_ROLLBACK,
                });
                match rollback(ctx, app, None) {
                    Ok(v) => {
                        let detail = json!({ "from_release": release, "reason": why, "result": v });
                        store.audit(Some(app), "watch.rollback", detail.clone());
                        events.push(ev("rollback", detail));
                    }
                    Err(e) => {
                        let detail = json!({ "from_release": release, "reason": why, "error": e.to_string() });
                        store.audit(Some(app), "watch.rollback_failed", detail.clone());
                        events.push(ev("rollback_failed", detail));
                    }
                }
                // The hook may have changed the deployment; only memory is ours to write.
                continue;
            }
            Decision::GaveUp => {
                if condition_changed {
                    store.audit(Some(app), "watch.gave_up", json!({ "container": container, "reason": "rollback already attempted for this release" }));
                }
                events.push(ev("gave_up", json!({ "seen": format!("{seen:?}") })));
            }
        }
        let _ = store.update(|st| {
            if let Some(d) = st.deployments.get_mut(app) {
                if d.current == Some(release) {
                    if reset_failures {
                        d.health_failures = 0;
                    }
                    if let Some(n) = set_failures {
                        d.health_failures = n;
                    }
                }
            }
            save_memory(st, app, &mem);
            Ok(())
        });
    }
    events
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_lock_is_exclusive_and_released_on_drop() {
        let home = std::env::temp_dir().join(format!("ltn-observe-lock-{}", uuid::Uuid::new_v4().simple()));
        let ctx = Ctx::for_home(&home);
        {
            let _held = AppLock::acquire(&ctx, "web", "test").unwrap();
            let err = AppLock::acquire(&ctx, "web", "test").unwrap_err();
            assert!(err.to_string().contains("busy"), "{err}");
            // ltn_repair must refuse while an apply (or anyone) holds the lock.
            let err = crate::repair(&ctx, "web").unwrap_err();
            assert!(err.to_string().contains("busy"), "{err}");
            // Other apps are unaffected.
            let _other = AppLock::acquire(&ctx, "api", "test").unwrap();
        }
        let st = ctx.store().load().unwrap();
        assert!(!is_locked(&st, "web", chrono::Utc::now()));
        assert!(!is_locked(&st, "api", chrono::Utc::now()));
        let _again = AppLock::acquire(&ctx, "web", "test").unwrap();
        drop(_again);
        let _ = std::fs::remove_dir_all(&home);
    }

    use super::*;

    fn mem(repairs: u32, rolled: bool) -> AppMemory {
        AppMemory { release: 2, repairs, rollback_attempted: rolled, condition: String::new() }
    }

    #[test]
    fn stopped_container_is_started_then_rolled_back_after_two_repairs() {
        let s = Seen::Stopped { exit_code: 1, restart_count: 0 };
        assert_eq!(decide(&s, 0, &mem(0, false)), Decision::Start);
        assert_eq!(decide(&s, 0, &mem(1, false)), Decision::Start);
        assert_eq!(decide(&s, 0, &mem(2, false)), Decision::Rollback);
        assert_eq!(decide(&s, 0, &mem(2, true)), Decision::GaveUp);
    }

    #[test]
    fn failing_probe_counts_then_restarts() {
        let s = Seen::Running { probe_ok: Some(false), restart_count: 0 };
        assert_eq!(decide(&s, 0, &mem(0, false)), Decision::CountFailure(1));
        assert_eq!(decide(&s, 1, &mem(0, false)), Decision::CountFailure(2));
        assert_eq!(decide(&s, 2, &mem(0, false)), Decision::Restart);
        assert_eq!(decide(&s, 2, &mem(2, false)), Decision::Rollback);
    }

    #[test]
    fn healthy_resets_and_restart_count_triggers_rollback() {
        assert_eq!(decide(&Seen::Running { probe_ok: Some(true), restart_count: 0 }, 2, &mem(1, false)), Decision::Healthy);
        assert_eq!(decide(&Seen::Running { probe_ok: None, restart_count: 0 }, 0, &mem(0, false)), Decision::Healthy);
        assert_eq!(decide(&Seen::Restarting { restart_count: 3 }, 0, &mem(0, false)), Decision::Rollback);
        assert_eq!(decide(&Seen::Running { probe_ok: None, restart_count: 4 }, 0, &mem(0, false)), Decision::Rollback);
        assert_eq!(decide(&Seen::Restarting { restart_count: 1 }, 0, &mem(0, false)), Decision::CountFailure(1));
    }

    #[test]
    fn missing_and_unreachable_are_reports() {
        assert_eq!(decide(&Seen::Missing, 0, &mem(0, false)), Decision::Report("container_missing"));
        assert_eq!(decide(&Seen::TargetUnreachable("x".into()), 0, &mem(0, false)), Decision::Report("target_unreachable"));
    }

    #[test]
    fn lock_expiry_is_respected() {
        let now = chrono::Utc::now();
        let mut st = State::default();
        st.ext.insert("locks".into(), json!({ "a": (now + chrono::Duration::minutes(5)).to_rfc3339(), "b": (now - chrono::Duration::minutes(5)).to_rfc3339() }));
        assert!(is_locked(&st, "a", now));
        assert!(!is_locked(&st, "b", now));
        assert!(!is_locked(&st, "c", now));
    }

    #[test]
    fn memory_resets_on_new_release() {
        let mut st = State::default();
        save_memory(&mut st, "a", &AppMemory { release: 1, repairs: 2, rollback_attempted: true, condition: "unhealthy".into() });
        assert_eq!(memory_of(&st, "a", 1).repairs, 2);
        let fresh = memory_of(&st, "a", 2);
        assert_eq!((fresh.repairs, fresh.rollback_attempted, fresh.condition.as_str()), (0, false, "unhealthy"));
    }
}
