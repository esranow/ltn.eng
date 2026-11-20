//! Observer and debugger: status, logs, diagnosis, and the watch loop.
//!
//! Everything reported here comes from the container runtime (`docker
//! inspect`, `docker stats`, `docker logs`, `docker info`) and a real HTTP
//! probe of the published port. Nothing defaults to healthy.

pub mod diagnose;
pub mod gather;
pub mod model;
pub mod watch;

use anyhow::{anyhow, bail, Result};
use ltn_state::tool::{arg_str, arg_str_opt, arg_u64_opt};
use ltn_state::{Ctx, ToolDef};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub use diagnose::{diagnose, gather_evidence, Evidence, Finding, Severity, SuggestedAction};
pub use model::{Probe, Stats, Verdict};
pub use watch::{decide, watch_tick, AppMemory, Decision, Seen, WatchEvent};

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ltn_status",
            description: "What is actually running: per app the current release, target, container state from `docker inspect`, a real HTTP probe of the published health URL, CPU/memory from `docker stats`, uptime, and a verdict (healthy | degraded | down | missing) with the reasons. Omit `app` for every deployment.",
            input_schema: json!({
                "type": "object",
                "properties": { "app": { "type": "string", "description": "App name; omit for all deployments" } }
            }),
            handler: status_tool,
        },
        ToolDef {
            name: "ltn_logs",
            description: "Recent stdout/stderr of an app's container (`docker logs`). Defaults to the current release and the last 100 lines.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": { "type": "string" },
                    "tail": { "type": "integer", "minimum": 1, "description": "Lines from the end (default 100)" },
                    "release": { "type": "integer", "minimum": 1, "description": "Release number (default: current)" }
                },
                "required": ["app"]
            }),
            handler: logs_tool,
        },
        ToolDef {
            name: "ltn_diagnose",
            description: "Explain why an app is unhealthy from real evidence (container state, exit code, OOM flag, last 200 log lines, HTTP probe, stats, spec, image presence, target reachability, recent audit). Returns findings with severity, cause, quoted evidence and a suggested next tool call.",
            input_schema: json!({
                "type": "object",
                "properties": { "app": { "type": "string" } },
                "required": ["app"]
            }),
            handler: diagnose_tool,
        },
        ToolDef {
            name: "ltn_repair",
            description: "Safe repair of the current release: `docker start` if the container is stopped, `docker restart` if it runs but is unhealthy, then re-probe. Returns before/after status. Fails if the container does not exist (use ltn_plan + ltn_apply or ltn_rollback).",
            input_schema: json!({
                "type": "object",
                "properties": { "app": { "type": "string" } },
                "required": ["app"]
            }),
            handler: repair_tool,
        },
        ToolDef {
            name: "ltn_audit",
            description: "Recent entries of the audit log (every state-changing action by tools and the watcher), oldest first.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "description": "Default 50" }
                }
            }),
            handler: audit_tool,
        },
    ]
}

/// What the watcher calls when restarting an app did not make it healthy.
/// Matches `ltn_engine::rollback`.
pub type RollbackHook = fn(&Ctx, &str, Option<u32>) -> Result<Value>;

/// Start the background watch loop: every `interval` it checks each
/// deployment, restarts what is down, and calls `rollback` when restarts fail.
/// A failing or panicking tick is caught; the loop keeps running.
pub fn spawn_watcher(ctx: Ctx, interval: Duration, rollback: RollbackHook) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("ltn-watcher".into())
        .spawn(move || loop {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| watch_tick(&ctx, rollback)));
            if res.is_err() {
                ctx.store().audit(None, "watch.tick_panicked", json!({}));
            }
            std::thread::sleep(interval);
        })
        .expect("spawning the ltn watcher thread")
}

/// `ltn_status` for one app, or all.
pub fn status(ctx: &Ctx, app: Option<&str>) -> Result<Value> {
    let state = ctx.store().load()?;
    let deps: Vec<_> = match app {
        Some(a) => vec![gather::load_deployment(ctx, a)?.1],
        None => state.deployments.values().cloned().collect(),
    };
    let apps: Vec<Value> = deps
        .iter()
        .map(|d| gather::observe(ctx, &state, d, None, true).to_json())
        .collect();
    Ok(json!({ "apps": apps, "count": apps.len(), "observed_at": chrono::Utc::now().to_rfc3339() }))
}

fn status_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    status(ctx, arg_str_opt(args, "app"))
}

fn logs_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = arg_str(args, "app")?;
    let tail = arg_u64_opt(args, "tail").unwrap_or(100).max(1) as usize;
    let (state, dep) = gather::load_deployment(ctx, app)?;
    let release = match arg_u64_opt(args, "release") {
        Some(r) => r as u32,
        None => dep
            .current
            .ok_or_else(|| anyhow!("`{app}` has no current release; pass `release` explicitly"))?,
    };
    let target = gather::find_target(&state, &dep.target_id).map_err(|e| anyhow!(e))?;
    let name = ltn_state::store::container_name(app, release);
    let docker = ctx.docker(&target);
    if docker.inspect(&name)?.is_none() {
        bail!(
            "container {name} does not exist on target {} (releases recorded: {}). Check ltn_status.",
            dep.target_id,
            dep.releases.iter().map(|r| format!("r{}={}", r.number, r.state)).collect::<Vec<_>>().join(", ")
        );
    }
    let logs = docker.logs(&name, tail)?;
    Ok(json!({
        "app": app,
        "release": release,
        "container": name,
        "target_id": dep.target_id,
        "tail": tail,
        "logs": logs,
    }))
}

fn diagnose_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = arg_str(args, "app")?;
    let (ev, _) = gather_evidence(ctx, app)?;
    let findings = diagnose(&ev);
    Ok(json!({
        "app": app,
        "verdict": ev.verdict().as_str(),
        "release": ev.release,
        "target_id": ev.target_id,
        "findings": findings,
        "evidence": {
            "container": ev.container,
            "published_ports": ev.published_ports,
            "memory_limit_mb": ev.memory_limit_mb,
            "probe": ev.probe,
            "stats": ev.stats,
            "image": ev.image,
            "image_exists": ev.image_exists,
            "target_error": ev.target_error,
            "previous_release": ev.previous_release,
            "log_lines": ev.logs.lines().count(),
        },
    }))
}

/// How long `ltn_repair` waits for the app to come back healthy.
fn repair_wait() -> Duration {
    let s = std::env::var("LTN_REPAIR_WAIT_S").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    Duration::from_secs(s)
}

pub fn repair(ctx: &Ctx, app: &str) -> Result<Value> {
    // Hold the app's lock so a concurrent apply/rollback cannot replace the
    // container while we start or restart it.
    let _lock = watch::AppLock::acquire(ctx, app, "ltn_repair")?;
    let (state, dep) = gather::load_deployment(ctx, app)?;
    let release = dep
        .current
        .ok_or_else(|| anyhow!("`{app}` has no current release to repair; deploy with ltn_plan + ltn_apply"))?;
    let before = gather::observe(ctx, &state, &dep, Some(release), false);
    if let Some(e) = &before.target_error {
        bail!("cannot repair `{app}`: target {} is unreachable: {e}", dep.target_id);
    }
    let name = before.container_name.clone().unwrap_or_default();
    let Some(c) = before.container.clone() else {
        let hint = match gather::previous_release(&dep) {
            Some(p) => format!("roll back with ltn_rollback {{\"app\": \"{app}\", \"to_release\": {p}}} or redeploy with ltn_plan + ltn_apply"),
            None => "redeploy with ltn_plan + ltn_apply".into(),
        };
        bail!("container {name} does not exist on target {}; nothing to repair. {hint}", dep.target_id);
    };
    let docker = before.docker(ctx).ok_or_else(|| anyhow!("target not found"))?;
    let (verdict_before, _) = before.verdict();
    let action = if !c.running {
        docker.run_ok(&["start", &name])?;
        "start"
    } else if verdict_before != Verdict::Healthy {
        docker.run_ok(&["restart", &name])?;
        "restart"
    } else {
        "none"
    };
    // Re-observe until healthy or the wait runs out.
    let deadline = Instant::now() + if action == "none" { Duration::ZERO } else { repair_wait() };
    let mut after = gather::observe(ctx, &state, &dep, Some(release), false);
    while after.verdict().0 != Verdict::Healthy && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        after = gather::observe(ctx, &state, &dep, Some(release), false);
    }
    let healthy = after.verdict().0 == Verdict::Healthy;
    let store = ctx.store();
    if action != "none" {
        store.audit(
            Some(app),
            "repair",
            json!({ "container": name, "action": action, "before": verdict_before.as_str(), "after": after.verdict().0.as_str() }),
        );
        if healthy {
            let _ = store.update(|st| {
                if let Some(d) = st.deployments.get_mut(app) {
                    if d.current == Some(release) {
                        d.health_failures = 0;
                    }
                }
                Ok(())
            });
        }
    }
    Ok(json!({
        "app": app,
        "release": release,
        "container": name,
        "action": action,
        "healthy": healthy,
        "before": before.to_json(),
        "after": after.to_json(),
        "next": if healthy { Value::Null } else { json!("still unhealthy: run ltn_diagnose for the cause") },
    }))
}

fn repair_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    repair(ctx, arg_str(args, "app")?)
}

fn audit_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = arg_str_opt(args, "app");
    let limit = arg_u64_opt(args, "limit").unwrap_or(50).max(1) as usize;
    let entries = ctx.store().recent_audit(app, limit);
    Ok(json!({ "app": app, "limit": limit, "count": entries.len(), "entries": entries }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltn_state::store::{Deployment, Release, State};

    /// With no usable Docker binary every tool reports the failure; nothing is healthy.
    #[test]
    fn missing_docker_is_reported_not_hidden() {
        let home = std::env::temp_dir().join(format!("ltn-observe-nodocker-{}", uuid::Uuid::new_v4().simple()));
        let mut ctx = Ctx::for_home(&home);
        ctx.docker_bin = "ltn-no-such-docker-binary".into();
        let spec: ltn_state::spec::AppSpec = serde_json::from_value(json!({
            "name": "a", "path": "/x", "dockerfile": "Dockerfile", "port": 8080, "host_port": 18299,
            "health_path": "/", "health_timeout_s": 5, "cpu_millis": 500, "ram_mb": 64, "gpu_count": 0,
            "vram_mb": 0, "max_cost_cents_per_hour": 1, "env": {}, "stack": [], "sources": {}
        }))
        .unwrap();
        let mut st = State::default();
        st.deployments.insert(
            "a".into(),
            Deployment {
                app: "a".into(),
                target_id: "local".into(),
                spec,
                releases: vec![Release { number: 1, image: "ltn/a:r1".into(), plan_id: None, created_at: chrono::Utc::now(), state: "active".into() }],
                current: Some(1),
                health_failures: 0,
            },
        );
        ctx.store().save(&st).unwrap();

        let s = status(&ctx, None).unwrap();
        assert_eq!(s["apps"][0]["verdict"], "missing");
        assert!(s["apps"][0]["target_error"].as_str().unwrap().contains("could not run"), "{s:#}");

        let d = (tools().into_iter().find(|t| t.name == "ltn_diagnose").unwrap().handler)(&ctx, &json!({"app": "a"})).unwrap();
        assert_eq!(d["verdict"], "missing");
        assert!(d["findings"][0]["cause"].as_str().unwrap().contains("unreachable"));

        assert!(repair(&ctx, "a").unwrap_err().to_string().contains("unreachable"));
        fn hook(_: &Ctx, _: &str, _: Option<u32>) -> Result<Value> {
            panic!("no rollback on an unreachable target")
        }
        let ev = watch_tick(&ctx, hook);
        assert_eq!(ev[0].kind, "target_unreachable");
        watch_tick(&ctx, hook);
        let audit = ctx.store().recent_audit(Some("a"), 10);
        assert_eq!(audit.len(), 1, "unreachable audited once per change: {audit:?}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_names_match_the_plan() {
        let names: Vec<_> = tools().iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["ltn_status", "ltn_logs", "ltn_diagnose", "ltn_repair", "ltn_audit"]);
    }
}
