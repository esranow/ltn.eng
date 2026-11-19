//! Integration tests against the real local Docker daemon. Run with
//! `cargo test -p ltn-observe -- --ignored --test-threads=1`.
//! Host ports used: 18200-18299.

use ltn_observe::{diagnose, gather_evidence, status, tools, watch_tick, Severity};
use ltn_state::spec::AppSpec;
use ltn_state::store::{container_name, Deployment, Release, State};
use ltn_state::Ctx;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HTTPD: &str = "mkdir -p /www && echo ok > /www/index.html && httpd -f -p 8080 -h /www";

fn docker(args: &[&str]) -> (bool, String) {
    let out = Command::new("docker").args(args).output().expect("docker CLI");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text.trim().to_string())
}

struct Fixture {
    ctx: Ctx,
    app: String,
    home: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for r in 1..=3 {
            let _ = docker(&["rm", "-f", &container_name(&self.app, r)]);
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Fixture {
    /// State with one deployment on the local target. `current` is the active
    /// release; releases below it are recorded as superseded.
    fn new(tag: &str, host_port: Option<u16>, ram_mb: u64, current: u32) -> Self {
        let app = format!("obs-{tag}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let home = std::env::temp_dir().join(format!("ltn-observe-it-{app}"));
        let ctx = Ctx::for_home(&home);
        let spec = AppSpec {
            name: app.clone(),
            path: home.to_string_lossy().to_string(),
            dockerfile: "Dockerfile".into(),
            port: host_port.map(|_| 8080),
            host_port,
            health_path: "/".into(),
            health_timeout_s: 30,
            cpu_millis: 500,
            ram_mb,
            gpu_count: 0,
            vram_mb: 0,
            max_cost_cents_per_hour: u64::MAX,
            env: BTreeMap::new(),
            stack: vec![],
            sources: BTreeMap::new(),
        };
        let releases = (1..=current)
            .map(|n| Release {
                number: n,
                image: "busybox:latest".into(),
                plan_id: None,
                created_at: chrono::Utc::now(),
                state: if n == current { "active".into() } else { "superseded".into() },
            })
            .collect();
        let mut st = State::default();
        st.deployments.insert(
            app.clone(),
            Deployment {
                app: app.clone(),
                target_id: "local".into(),
                spec,
                releases,
                current: Some(current),
                health_failures: 0,
            },
        );
        ctx.store().save(&st).unwrap();
        Fixture { ctx, app, home }
    }

    fn container(&self, release: u32) -> String {
        container_name(&self.app, release)
    }

    /// `docker run -d` with ltn's naming and labels; `extra` goes before the image.
    fn run(&self, release: u32, extra: &[&str], cmd: &str) {
        let name = self.container(release);
        let app_label = format!("ltn.app={}", self.app);
        let rel_label = format!("ltn.release={release}");
        let mut args = vec!["run", "-d", "--name", &name, "--label", &app_label, "--label", &rel_label];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["busybox:latest", "sh", "-c", cmd]);
        let (ok, out) = docker(&args);
        assert!(ok, "docker run failed: {out}");
    }

    fn app_status(&self) -> Value {
        status(&self.ctx, Some(&self.app)).unwrap()["apps"][0].clone()
    }

    fn wait_verdict(&self, want: &str, secs: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let s = self.app_status();
            if s["verdict"] == want || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn tool(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        let t = tools().into_iter().find(|t| t.name == name).unwrap();
        (t.handler)(&self.ctx, &args)
    }
}

fn wait_exited(name: &str) {
    let (ok, out) = docker(&["wait", name]);
    assert!(ok, "docker wait: {out}");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn status_reports_healthy_from_real_probe_and_stats() {
    let f = Fixture::new("status", Some(18201), 64, 1);
    f.run(1, &["-p", "18201:8080", "--memory", "64m"], HTTPD);
    let s = f.wait_verdict("healthy", 20);
    assert_eq!(s["verdict"], "healthy", "{s:#}");
    assert_eq!(s["probe"]["status"], 200, "{s:#}");
    assert_eq!(s["probe"]["url"], "http://127.0.0.1:18201/");
    assert_eq!(s["container"]["running"], true);
    assert_eq!(s["stats"]["memory_limit_mb"], 64.0, "{s:#}");
    assert!(s["stats"]["memory_used_mb"].as_f64().unwrap() > 0.0, "{s:#}");
    assert!(s["uptime_s"].as_i64().is_some());
    assert_eq!(s["container"]["published_ports"][0], "8080/tcp -> 0.0.0.0:18201");

    let d = f.tool("ltn_diagnose", json!({ "app": f.app })).unwrap();
    assert_eq!(d["verdict"], "healthy", "{d:#}");
    assert!(d["findings"][0]["cause"].as_str().unwrap().contains("no problem found"), "{d:#}");

    // Unknown app is an error naming the known ones.
    let err = status(&f.ctx, Some("nope")).unwrap_err().to_string();
    assert!(err.contains(&f.app), "{err}");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn logs_returns_container_output() {
    let f = Fixture::new("logs", None, 32, 1);
    let marker = format!("hello-from-{}", f.app);
    f.run(1, &[], &format!("echo {marker}; echo second line >&2; sleep 300"));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut out;
    loop {
        out = f.tool("ltn_logs", json!({ "app": f.app, "tail": 10 })).unwrap();
        if out["logs"].as_str().unwrap().contains("second line") || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    assert_eq!(out["release"], 1);
    assert_eq!(out["container"], f.container(1));
    let logs = out["logs"].as_str().unwrap();
    assert!(logs.contains(&marker) && logs.contains("second line"), "{logs}");
    // A release with no container is a clear error.
    let err = f.tool("ltn_logs", json!({ "app": f.app, "release": 2 })).unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
}

fn no_rollback(_: &Ctx, app: &str, _: Option<u32>) -> anyhow::Result<Value> {
    panic!("rollback must not be called for {app}");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn watcher_starts_a_stopped_container() {
    let f = Fixture::new("watch", Some(18202), 64, 1);
    f.run(1, &["-p", "18202:8080", "--restart", "unless-stopped"], HTTPD);
    assert_eq!(f.wait_verdict("healthy", 20)["verdict"], "healthy");
    let (ok, out) = docker(&["stop", "-t", "1", &f.container(1)]);
    assert!(ok, "{out}");
    assert_eq!(f.app_status()["verdict"], "down");

    let events = watch_tick(&f.ctx, no_rollback);
    let mine: Vec<_> = events.iter().filter(|e| e.app == f.app).collect();
    assert_eq!(mine.len(), 1, "{events:?}");
    assert_eq!(mine[0].kind, "started", "{events:?}");
    let s = f.wait_verdict("healthy", 20);
    assert_eq!(s["verdict"], "healthy", "{s:#}");
    assert_eq!(s["probe"]["status"], 200);

    // Next tick sees it healthy and clears the repair count.
    let events = watch_tick(&f.ctx, no_rollback);
    assert_eq!(events.iter().find(|e| e.app == f.app).unwrap().kind, "healthy");
    let audit = f.ctx.store().recent_audit(Some(&f.app), 10);
    let actions: Vec<_> = audit.iter().map(|e| e["action"].as_str().unwrap().to_string()).collect();
    assert!(actions.contains(&"watch.start".into()) && actions.contains(&"watch.recovered".into()), "{actions:?}");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn repair_starts_a_stopped_container_and_errors_when_missing() {
    let f = Fixture::new("repair", Some(18203), 64, 1);
    f.run(1, &["-p", "18203:8080"], HTTPD);
    assert_eq!(f.wait_verdict("healthy", 20)["verdict"], "healthy");
    docker(&["stop", "-t", "1", &f.container(1)]);
    let r = f.tool("ltn_repair", json!({ "app": f.app })).unwrap();
    assert_eq!(r["action"], "start", "{r:#}");
    assert_eq!(r["before"]["verdict"], "down");
    assert_eq!(r["after"]["verdict"], "healthy", "{r:#}");
    assert_eq!(f.ctx.store().recent_audit(Some(&f.app), 5).last().unwrap()["action"], "repair");

    docker(&["rm", "-f", &f.container(1)]);
    let err = f.tool("ltn_repair", json!({ "app": f.app })).unwrap_err().to_string();
    assert!(err.contains("does not exist") && err.contains("ltn_plan"), "{err}");
    assert_eq!(f.app_status()["verdict"], "missing");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn diagnose_reports_nonzero_exit_and_quotes_log() {
    let f = Fixture::new("exit", None, 32, 1);
    f.run(1, &[], "echo booting; echo 'fatal: CONFIG_PATH is not set' >&2; exit 3");
    wait_exited(&f.container(1));
    let (ev, _) = gather_evidence(&f.ctx, &f.app).unwrap();
    let findings = diagnose(&ev);
    let x = findings
        .iter()
        .find(|x| x.cause.contains("exited with code 3"))
        .unwrap_or_else(|| panic!("{findings:#?}"));
    assert_eq!(x.severity, Severity::Critical);
    assert!(x.evidence.iter().any(|e| e.contains("fatal: CONFIG_PATH is not set")), "{x:#?}");
    assert!(x.evidence.iter().any(|e| e.contains("ExitCode=3")), "{x:#?}");
    let d = f.tool("ltn_diagnose", json!({ "app": f.app })).unwrap();
    assert_eq!(d["verdict"], "down");
    assert_eq!(d["evidence"]["image_exists"], true);
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn diagnose_reports_oom_kill() {
    let f = Fixture::new("oom", None, 16, 1);
    f.run(1, &["--memory", "16m", "--memory-swap", "16m"], "tail /dev/zero");
    wait_exited(&f.container(1));
    let (ev, _) = gather_evidence(&f.ctx, &f.app).unwrap();
    let c = ev.container.clone().unwrap();
    assert_eq!(c.exit_code, 137);
    // Docker Desktop 28 on this machine sets OOMKilled reliably for this case.
    assert!(c.oom_killed, "docker did not set OOMKilled: {c:?}");
    let findings = diagnose(&ev);
    let x = findings.iter().find(|x| x.cause.contains("OOMKilled")).unwrap_or_else(|| panic!("{findings:#?}"));
    assert!(x.evidence.iter().any(|e| e.contains("container memory limit 16 MB")), "{x:#?}");
    let a = x.suggested_action.clone().unwrap();
    assert_eq!(a.tool, "ltn_plan");
    assert!(a.why.contains("ram_mb") && a.why.contains("32 MB"), "{a:?}");
}

static ROLLBACK_CALLS: Mutex<Vec<(String, Option<u32>)>> = Mutex::new(Vec::new());

fn recording_rollback(_: &Ctx, app: &str, to: Option<u32>) -> anyhow::Result<Value> {
    ROLLBACK_CALLS.lock().unwrap().push((app.to_string(), to));
    Ok(json!({ "app": app, "release": 1, "from_release": 2, "health": "test hook" }))
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn failing_release_is_rolled_back_exactly_once() {
    let f = Fixture::new("rollback", None, 32, 2);
    f.run(2, &["--restart", "no"], "echo dying; exit 1");
    wait_exited(&f.container(2));
    let mut kinds = Vec::new();
    for _ in 0..5 {
        let events = watch_tick(&f.ctx, recording_rollback);
        kinds.push(events.iter().find(|e| e.app == f.app).unwrap().kind.clone());
        // Give a started container time to exit again before the next look.
        std::thread::sleep(Duration::from_millis(1500));
    }
    assert_eq!(kinds, vec!["started", "started", "rollback", "gave_up", "gave_up"], "{kinds:?}");
    let calls: Vec<_> = ROLLBACK_CALLS.lock().unwrap().iter().filter(|(a, _)| *a == f.app).cloned().collect();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let audit = f.ctx.store().recent_audit(Some(&f.app), 20);
    let actions: Vec<_> = audit.iter().map(|e| e["action"].as_str().unwrap().to_string()).collect();
    assert_eq!(actions.iter().filter(|a| *a == "watch.start").count(), 2, "{actions:?}");
    assert_eq!(actions.iter().filter(|a| *a == "watch.rollback").count(), 1, "{actions:?}");
    assert_eq!(actions.iter().filter(|a| *a == "watch.gave_up").count(), 1, "{actions:?}");
    // Diagnose points at the previous release.
    let (ev, _) = gather_evidence(&f.ctx, &f.app).unwrap();
    assert_eq!(ev.previous_release, Some(1));
    assert!(diagnose(&ev)
        .iter()
        .any(|x| x.suggested_action.as_ref().map_or(false, |a| a.tool == "ltn_rollback" && a.arguments["to_release"] == 1)));
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn crash_looping_container_is_rolled_back_on_restart_count() {
    let f = Fixture::new("loop", None, 32, 2);
    f.run(2, &["--restart", "unless-stopped"], "echo crash; exit 2");
    // Wait for docker's restart policy to reach RestartCount >= 3.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (_, out) = docker(&["inspect", "--format", "{{.RestartCount}}", &f.container(2)]);
        if out.parse::<i64>().unwrap_or(0) >= 3 || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let events = watch_tick(&f.ctx, recording_rollback);
    let e = events.iter().find(|e| e.app == f.app).unwrap();
    assert_eq!(e.kind, "rollback", "{events:?}");
    let events = watch_tick(&f.ctx, recording_rollback);
    assert_ne!(events.iter().find(|e| e.app == f.app).unwrap().kind, "rollback");
    let calls = ROLLBACK_CALLS.lock().unwrap().iter().filter(|(a, _)| *a == f.app).count();
    assert_eq!(calls, 1);
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn locked_app_is_skipped() {
    let f = Fixture::new("locked", None, 32, 1);
    f.run(1, &[], "sleep 300");
    docker(&["stop", "-t", "0", &f.container(1)]);
    let until = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
    f.ctx
        .store()
        .update(|st| {
            st.ext.insert("locks".into(), json!({ f.app.clone(): until }));
            Ok(())
        })
        .unwrap();
    let events = watch_tick(&f.ctx, no_rollback);
    assert_eq!(events.iter().find(|e| e.app == f.app).unwrap().kind, "skipped_locked");
    assert_eq!(f.app_status()["verdict"], "down", "watcher touched a locked app");

    // An expired lock no longer protects it.
    let past = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    f.ctx
        .store()
        .update(|st| {
            st.ext.insert("locks".into(), json!({ f.app.clone(): past }));
            Ok(())
        })
        .unwrap();
    let events = watch_tick(&f.ctx, no_rollback);
    assert_eq!(events.iter().find(|e| e.app == f.app).unwrap().kind, "started");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn missing_container_is_reported_once() {
    let f = Fixture::new("missing", None, 32, 1);
    for _ in 0..3 {
        let events = watch_tick(&f.ctx, no_rollback);
        assert_eq!(events.iter().find(|e| e.app == f.app).unwrap().kind, "container_missing");
    }
    let audit = f.ctx.store().recent_audit(Some(&f.app), 10);
    assert_eq!(audit.len(), 1, "{audit:?}");
    assert_eq!(audit[0]["action"], "watch.container_missing");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn running_but_not_listening_on_declared_port() {
    let f = Fixture::new("noport", Some(18204), 32, 1);
    // Port published, but nothing inside listens on 8080.
    f.run(1, &["-p", "18204:8080"], "echo started without a server; sleep 300");
    std::thread::sleep(Duration::from_millis(1500));
    let (ev, _) = gather_evidence(&f.ctx, &f.app).unwrap();
    let probe = ev.probe.clone().expect("probe made");
    assert!(!probe.ok, "{probe:?}");
    println!("REAL PROBE RESULT for a published port with no listener: {probe:?}");
    let findings = diagnose(&ev);
    let x = findings
        .iter()
        .find(|x| x.cause.contains("not listening on that port"))
        .unwrap_or_else(|| panic!("{findings:#?}"));
    assert!(x.evidence.iter().any(|e| e.contains("port = 8080, host_port = 18204")), "{x:#?}");
    assert!(x.evidence.iter().any(|e| e.contains("8080/tcp -> 0.0.0.0:18204")), "{x:#?}");
    assert_eq!(f.app_status()["verdict"], "degraded");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn wrong_health_path_is_reported_as_4xx() {
    let f = Fixture::new("hpath", Some(18205), 32, 1);
    f.ctx
        .store()
        .update(|st| {
            st.deployments.values_mut().for_each(|d| d.spec.health_path = "/healthz".into());
            Ok(())
        })
        .unwrap();
    f.run(1, &["-p", "18205:8080"], HTTPD);
    let s = f.wait_verdict("degraded", 15);
    assert_eq!(s["probe"]["status"], 404, "{s:#}");
    let d = f.tool("ltn_diagnose", json!({ "app": f.app })).unwrap();
    let causes: Vec<_> = d["findings"].as_array().unwrap().iter().map(|x| x["cause"].as_str().unwrap().to_string()).collect();
    assert!(causes.iter().any(|c| c.contains("/healthz returns HTTP 404")), "{causes:?}");
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn failing_probe_is_counted_then_restarted() {
    let f = Fixture::new("probefail", Some(18206), 32, 1);
    f.run(1, &["-p", "18206:8080"], "sleep 300");
    std::thread::sleep(Duration::from_millis(1000));
    let failures = |f: &Fixture| f.ctx.store().load().unwrap().deployments[&f.app].health_failures;
    let kind = |f: &Fixture| watch_tick(&f.ctx, no_rollback).into_iter().find(|e| e.app == f.app).unwrap().kind;
    assert_eq!(kind(&f), "probe_failed");
    assert_eq!(failures(&f), 1);
    assert_eq!(kind(&f), "probe_failed");
    assert_eq!(failures(&f), 2);
    assert_eq!(kind(&f), "restarted");
    assert_eq!(failures(&f), 0);
    let audit = f.ctx.store().recent_audit(Some(&f.app), 5);
    assert_eq!(audit.last().unwrap()["action"], "watch.restart", "{audit:?}");
}
