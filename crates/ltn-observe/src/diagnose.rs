//! Evidence-based diagnosis. `gather` collects facts from the runtime;
//! `diagnose` is a pure function from those facts to findings.

use crate::gather::{self, Observation};
use crate::model::{self, Probe, Stats, Verdict, MEMORY_PRESSURE_PERCENT};
use ltn_state::docker::ContainerState;
use ltn_state::spec::AppSpec;
use ltn_state::Ctx;
use serde::Serialize;
use serde_json::{json, Value};

/// Facts about one app, all read from the runtime (or the state file).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Evidence {
    pub app: String,
    pub release: Option<u32>,
    pub target_id: String,
    pub container_name: String,
    /// Set when the target's Docker daemon did not answer.
    pub target_error: Option<String>,
    pub container: Option<ContainerState>,
    pub published_ports: Vec<String>,
    pub memory_limit_mb: Option<u64>,
    /// Last 200 lines of `docker logs`.
    pub logs: String,
    pub probe: Option<Probe>,
    pub stats: Option<Stats>,
    pub spec: Option<AppSpec>,
    pub image: Option<String>,
    pub image_exists: Option<bool>,
    pub previous_release: Option<u32>,
    pub recent_audit: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SuggestedAction {
    pub tool: String,
    pub arguments: Value,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub cause: String,
    pub evidence: Vec<String>,
    pub suggested_action: Option<SuggestedAction>,
}

fn finding(severity: Severity, cause: impl Into<String>, evidence: Vec<String>, action: Option<SuggestedAction>) -> Finding {
    Finding { severity, cause: cause.into(), evidence, suggested_action: action }
}

fn action(tool: &str, arguments: Value, why: impl Into<String>) -> Option<SuggestedAction> {
    Some(SuggestedAction { tool: tool.into(), arguments, why: why.into() })
}

impl Evidence {
    pub fn verdict(&self) -> Verdict {
        if self.release.is_none() {
            return Verdict::Missing;
        }
        model::verdict(
            self.target_error.as_deref(),
            self.container.as_ref(),
            self.probe.as_ref(),
            self.stats.as_ref(),
        )
        .0
    }

    fn plan_action(&self, why: impl Into<String>) -> Option<SuggestedAction> {
        let path = self.spec.as_ref().map(|s| s.path.clone());
        action("ltn_plan", json!({ "path": path }), why)
    }

    fn last_log_lines(&self, n: usize) -> Vec<String> {
        let lines: Vec<&str> = self.logs.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(n)..]
            .iter()
            .map(|l| format!("log: {}", clip(l)))
            .collect()
    }

    fn ram_mb(&self) -> Option<u64> {
        self.spec.as_ref().map(|s| s.ram_mb)
    }

    fn limit_text(&self) -> String {
        match (self.memory_limit_mb, self.ram_mb()) {
            (Some(l), Some(r)) => format!("container memory limit {l} MB (HostConfig.Memory); ltn.toml ram_mb = {r}"),
            (Some(l), None) => format!("container memory limit {l} MB (HostConfig.Memory)"),
            (None, Some(r)) => format!("no memory limit on the container; spec ram_mb = {r}"),
            (None, None) => "no memory limit on the container".into(),
        }
    }
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 300 {
        format!("{}...", s.chars().take(300).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Log patterns worth a finding: (needles, cause, severity).
const LOG_PATTERNS: &[(&[&str], &str, Severity)] = &[
    (&["panicked at", "panic:"], "the application panicked (see the quoted log lines)", Severity::Critical),
    (&["traceback (most recent call last)"], "the application raised an uncaught Python exception", Severity::Critical),
    (
        &["address already in use", "eaddrinuse"],
        "the app tried to bind a port that is already in use inside the container (two listeners on one port, or a duplicate process)",
        Severity::Critical,
    ),
    (&["permission denied"], "a file or socket operation was refused with \"permission denied\" (file modes, user, or a privileged port < 1024 as non-root)", Severity::Warning),
    (&["no such file or directory"], "the app referenced a file or directory that does not exist in the image", Severity::Warning),
    (
        &["econnrefused", "connection refused"],
        "the app could not reach a dependency (connection refused): a database or service it needs is not running or not reachable from the container",
        Severity::Warning,
    ),
    (
        &["out of memory", "outofmemoryerror", "cannot allocate memory", "memoryerror"],
        "the app reported running out of memory",
        Severity::Warning,
    ),
];

/// Pure diagnosis: findings ordered critical first. Always returns at least one finding.
pub fn diagnose(ev: &Evidence) -> Vec<Finding> {
    let mut out = Vec::new();
    let app = ev.app.as_str();
    let prev = ev.previous_release;

    // 1. Target unreachable: nothing else can be known.
    if let Some(e) = &ev.target_error {
        out.push(finding(
            Severity::Critical,
            format!("target `{}` is unreachable, so the container cannot be inspected", ev.target_id),
            vec![format!("docker info / inspect on target {}: {}", ev.target_id, clip(e))],
            action("ltn_targets", json!({}), "check which targets answer and why this one does not (SSH access, Docker daemon running)"),
        ));
        return out;
    }

    if ev.release.is_none() {
        out.push(finding(
            Severity::Critical,
            "the deployment has no current release",
            vec![format!("state: deployments[{app}].current = null")],
            ev.plan_action("plan and apply a release"),
        ));
        return out;
    }

    // 2. Image missing.
    if ev.image_exists == Some(false) {
        let img = ev.image.clone().unwrap_or_default();
        out.push(finding(
            if ev.container.is_none() { Severity::Critical } else { Severity::Warning },
            format!("the release image {img} no longer exists on the target"),
            vec![format!("docker image inspect {img}: not found on target {}", ev.target_id)],
            match prev {
                Some(p) if ev.container.is_none() => action("ltn_rollback", json!({ "app": app, "to_release": p }), format!("release r{p} may still have its image")),
                _ => ev.plan_action("rebuild the image with ltn_plan + ltn_apply"),
            },
        ));
    }

    // 3. Container missing.
    let Some(c) = &ev.container else {
        out.push(finding(
            Severity::Critical,
            format!("no container {} exists on target {}", ev.container_name, ev.target_id),
            vec![format!("docker inspect {}: No such container", ev.container_name)],
            match prev {
                Some(p) => action("ltn_rollback", json!({ "app": app, "to_release": p }), format!("bring back release r{p}")),
                None => ev.plan_action("redeploy with ltn_plan + ltn_apply"),
            },
        ));
        add_rollback(ev, &mut out);
        return finish(out);
    };

    let state_line = format!(
        "docker inspect: Status={}, Running={}, Restarting={}, ExitCode={}, OOMKilled={}, RestartCount={}",
        c.status, c.running, c.restarting, c.exit_code, c.oom_killed, c.restart_count
    );

    // 4. Exit causes (container stopped, or crash-looping with a recorded exit).
    let exited = !c.running || c.restarting;
    if c.oom_killed {
        let ram = ev.ram_mb().unwrap_or(0);
        let mut why = format!("raise ram_mb in ltn.toml (currently {ram} MB");
        if ram > 0 {
            why.push_str(&format!(", e.g. to {} MB", ram * 2));
        }
        why.push_str("), then ltn_plan + ltn_apply");
        out.push(finding(
            Severity::Critical,
            "the container was killed for exceeding its memory limit (OOMKilled)",
            vec![state_line.clone(), ev.limit_text()],
            ev.plan_action(why),
        ));
    } else if exited && c.exit_code == 137 {
        out.push(finding(
            Severity::Critical,
            "the process was killed with SIGKILL (exit 137) without the OOM flag: killed from outside (docker kill, a stop timeout, or the host's own OOM killer)",
            vec![state_line.clone(), ev.limit_text()],
            action("ltn_repair", json!({ "app": app }), "start it again; if it recurs, check host memory and who stops the container"),
        ));
    } else if exited && c.exit_code == 126 {
        let mut evid = vec![state_line.clone()];
        evid.extend(ev.last_log_lines(5));
        out.push(finding(
            Severity::Critical,
            "the container command could not be executed (exit 126: not executable, wrong format, or permission denied)",
            evid,
            ev.plan_action("fix the CMD/ENTRYPOINT (chmod +x, correct interpreter or architecture) in the Dockerfile, then ltn_plan + ltn_apply"),
        ));
    } else if exited && c.exit_code == 127 {
        let mut evid = vec![state_line.clone()];
        evid.extend(ev.last_log_lines(5));
        out.push(finding(
            Severity::Critical,
            "the container command was not found (exit 127)",
            evid,
            ev.plan_action("fix the CMD/ENTRYPOINT in the Dockerfile or install the missing binary, then ltn_plan + ltn_apply"),
        ));
    } else if exited && c.exit_code != 0 {
        let mut evid = vec![state_line.clone()];
        if !c.error.is_empty() {
            evid.push(format!("docker State.Error: {}", c.error));
        }
        evid.extend(ev.last_log_lines(5));
        out.push(finding(
            Severity::Critical,
            format!("the app process exited with code {}", c.exit_code),
            evid,
            action("ltn_logs", json!({ "app": app, "tail": 200 }), "read the full output around the failure"),
        ));
    } else if !c.running && c.exit_code == 0 {
        let mut evid = vec![state_line.clone()];
        evid.extend(ev.last_log_lines(5));
        let stopped_by_user = c.status == "created";
        out.push(finding(
            Severity::Critical,
            if stopped_by_user {
                "the container was created but never started".to_string()
            } else {
                "the container is stopped with exit code 0: it was stopped, or its main command finished (a server must stay in the foreground)".to_string()
            },
            evid,
            action("ltn_repair", json!({ "app": app }), "start the container"),
        ));
    }

    // 5. Crash loop.
    if c.restarting || c.restart_count >= 3 {
        let mut evid = vec![state_line.clone()];
        evid.extend(ev.last_log_lines(5));
        out.push(finding(
            Severity::Critical,
            format!("the container is crash-looping (docker restarted it {} times)", c.restart_count),
            evid,
            match prev {
                Some(p) => action("ltn_rollback", json!({ "app": app, "to_release": p }), format!("release r{p} was the last superseded release")),
                None => action("ltn_logs", json!({ "app": app, "tail": 200 }), "find why each start fails"),
            },
        ));
    }

    // 6. Running: probe results.
    if c.running && !c.restarting {
        if let Some(p) = &ev.probe {
            let spec_port = ev.spec.as_ref().and_then(|s| s.port);
            let host_port = ev.spec.as_ref().and_then(|s| s.host_port.or(s.port));
            let probe_line = match (&p.status, &p.error) {
                (Some(s), _) => format!("GET {} -> HTTP {s} in {} ms", p.url, p.latency_ms),
                (None, Some(e)) => format!("GET {} -> {} after {} ms: {}", p.url, p.kind, p.latency_ms, clip(e)),
                _ => format!("GET {} failed", p.url),
            };
            let ports_line = format!(
                "published ports (docker inspect): {}",
                if ev.published_ports.is_empty() { "none".to_string() } else { ev.published_ports.join(", ") }
            );
            match p.kind.as_str() {
                "refused" | "reset" => {
                    let mut evid = vec![
                        probe_line,
                        format!(
                            "spec: port = {}, host_port = {}",
                            spec_port.map_or("none".into(), |v| v.to_string()),
                            host_port.map_or("none".into(), |v| v.to_string())
                        ),
                        ports_line,
                    ];
                    evid.extend(
                        ev.logs
                            .lines()
                            .filter(|l| l.to_lowercase().contains("listen"))
                            .take(3)
                            .map(|l| format!("log: {}", clip(l))),
                    );
                    out.push(finding(
                        Severity::Critical,
                        format!(
                            "the container is running but nothing answers on the declared port {}: the app is not listening on that port (or not on 0.0.0.0)",
                            spec_port.map_or("?".into(), |v| v.to_string())
                        ),
                        evid,
                        ev.plan_action("set `port` in ltn.toml to the port the app really listens on (bind 0.0.0.0), then ltn_plan + ltn_apply"),
                    ));
                }
                "timeout" => out.push(finding(
                    Severity::Critical,
                    "the health probe timed out: the app accepted the connection or the port is filtered, but no response came back in time",
                    vec![probe_line, ports_line],
                    action("ltn_repair", json!({ "app": app }), "restart the container if it is hung; check logs for a blocked startup"),
                )),
                "http_status" => {
                    let s = p.status.unwrap_or(0);
                    if s >= 500 {
                        let mut evid = vec![probe_line];
                        evid.extend(ev.last_log_lines(5));
                        out.push(finding(
                            Severity::Critical,
                            format!("the app answers its health path with HTTP {s} (server error)"),
                            evid,
                            action("ltn_logs", json!({ "app": app, "tail": 200 }), "find the error the app logs for the failing request"),
                        ));
                    } else if s >= 400 {
                        let hp = ev.spec.as_ref().map(|s| s.health_path.clone()).unwrap_or_default();
                        out.push(finding(
                            Severity::Warning,
                            format!("the health path {hp} returns HTTP {s}; the app is up but this path is not a health endpoint"),
                            vec![probe_line],
                            ev.plan_action("set `health_path` in ltn.toml to a path that returns 2xx, then ltn_plan + ltn_apply"),
                        ));
                    }
                }
                "error" => out.push(finding(
                    Severity::Warning,
                    "the health probe failed with a transport error",
                    vec![probe_line],
                    None,
                )),
                _ => {}
            }
        }
        // 7. Memory pressure.
        if let Some(s) = &ev.stats {
            if s.memory_limit_mb > 0.0 && s.memory_percent > MEMORY_PRESSURE_PERCENT {
                let ram = ev.ram_mb().unwrap_or(0);
                out.push(finding(
                    Severity::Warning,
                    format!("memory use is above {MEMORY_PRESSURE_PERCENT}% of the limit; the container is close to being OOM-killed"),
                    vec![format!("docker stats: MemUsage {} ({}%)", s.raw_mem_usage, s.memory_percent), ev.limit_text()],
                    ev.plan_action(format!("raise ram_mb in ltn.toml (currently {ram} MB), then ltn_plan + ltn_apply")),
                ));
            }
        }
    }

    // 8. Log patterns.
    let lower: Vec<(String, &str)> = ev.logs.lines().map(|l| (l.to_lowercase(), l)).collect();
    for (needles, cause, sev) in LOG_PATTERNS {
        let hits: Vec<String> = lower
            .iter()
            .filter(|(lc, _)| needles.iter().any(|n| lc.contains(n)))
            .map(|(_, l)| format!("log: {}", clip(l)))
            .collect();
        if !hits.is_empty() {
            let mut evid: Vec<String> = hits.iter().take(3).cloned().collect();
            if hits.len() > 3 {
                evid.push(format!("({} matching lines in the last 200)", hits.len()));
            }
            out.push(finding(*sev, *cause, evid, None));
        }
    }

    // 9. Recent watcher / repair activity is context worth surfacing.
    let repairs: Vec<String> = ev
        .recent_audit
        .iter()
        .filter(|e| {
            let a = e["action"].as_str().unwrap_or("");
            a.starts_with("watch.") || a == "repair"
        })
        .map(|e| format!("audit {} {}", e["at"].as_str().unwrap_or(""), e["action"].as_str().unwrap_or("")))
        .collect();
    if !repairs.is_empty() {
        let n = repairs.len();
        out.push(finding(
            Severity::Info,
            format!("the watcher or ltn_repair acted on this app {n} time(s) recently"),
            repairs.into_iter().rev().take(5).collect(),
            None,
        ));
    }

    add_rollback(ev, &mut out);
    finish_with_healthy(ev, out)
}

fn add_rollback(ev: &Evidence, out: &mut Vec<Finding>) {
    let failing = !matches!(ev.verdict(), Verdict::Healthy) && ev.target_error.is_none();
    if let (true, Some(p)) = (failing, ev.previous_release) {
        if out.iter().any(|f| f.suggested_action.as_ref().map_or(false, |a| a.tool == "ltn_rollback")) {
            return;
        }
        out.push(finding(
            Severity::Warning,
            format!("release r{} is failing and the previous release r{p} is available", ev.release.unwrap_or(0)),
            vec![format!("verdict {}; state: release r{p} is superseded", ev.verdict().as_str())],
            action("ltn_rollback", json!({ "app": ev.app, "to_release": p }), format!("return to the last known release r{p} while the current one is fixed")),
        ));
    }
}

fn finish(mut out: Vec<Finding>) -> Vec<Finding> {
    out.sort_by_key(|f| f.severity);
    out
}

fn finish_with_healthy(ev: &Evidence, out: Vec<Finding>) -> Vec<Finding> {
    let problems = out.iter().any(|f| f.severity != Severity::Info);
    let mut out = out;
    if !problems && ev.verdict() == Verdict::Healthy {
        let c = ev.container.as_ref();
        let mut evid = Vec::new();
        if let Some(c) = c {
            evid.push(format!(
                "docker inspect: Status={}, StartedAt={}, RestartCount={}",
                c.status, c.started_at, c.restart_count
            ));
        }
        match &ev.probe {
            Some(p) => evid.push(format!("GET {} -> HTTP {} in {} ms", p.url, p.status.unwrap_or(0), p.latency_ms)),
            None => evid.push("spec declares no port; health = container running".into()),
        }
        if let Some(s) = &ev.stats {
            evid.push(format!("docker stats: CPU {}%, MemUsage {} ({}%)", s.cpu_percent, s.raw_mem_usage, s.memory_percent));
        }
        if ev.image_exists == Some(true) {
            evid.push(format!("image {} present", ev.image.clone().unwrap_or_default()));
        }
        out.insert(0, finding(Severity::Info, "no problem found: the app is running and healthy", evid, None));
    } else if out.is_empty() {
        // Not healthy yet no rule matched: say what is known rather than nothing.
        out.push(finding(
            Severity::Warning,
            "the app is not healthy but no specific cause was identified",
            vec![format!("verdict {}", ev.verdict().as_str())],
            action("ltn_logs", json!({ "app": ev.app, "tail": 200 }), "read the full output"),
        ));
    }
    finish(out)
}

/// Collects evidence for `app` from the real runtime.
pub fn gather_evidence(ctx: &Ctx, app: &str) -> anyhow::Result<(Evidence, Observation)> {
    let (state, dep) = gather::load_deployment(ctx, app)?;
    let store = ctx.store();
    let mut ev = Evidence {
        app: app.to_string(),
        release: dep.current,
        target_id: dep.target_id.clone(),
        container_name: dep.current.map(|r| ltn_state::store::container_name(app, r)).unwrap_or_default(),
        spec: Some(dep.spec.clone()),
        image: dep.current_release().map(|r| r.image.clone()),
        previous_release: gather::previous_release(&dep),
        recent_audit: store.recent_audit(Some(app), 20),
        ..Default::default()
    };
    // Ask the target directly whether it answers.
    let target = gather::find_target(&state, &dep.target_id);
    if let Ok(t) = &target {
        if let Err(e) = ctx.docker(t).info() {
            ev.target_error = Some(e.to_string());
        }
    } else if let Err(e) = &target {
        ev.target_error = Some(e.clone());
    }
    let obs = gather::observe(ctx, &state, &dep, None, true);
    if ev.target_error.is_none() {
        ev.target_error = obs.target_error.clone();
    }
    if ev.target_error.is_none() {
        if let (Some(t), Some(name)) = (&obs.target, &obs.container_name) {
            let docker = ctx.docker(t);
            if obs.container.is_some() {
                ev.logs = docker.logs(name, 200).unwrap_or_else(|e| format!("(docker logs failed: {e})"));
            }
            if let Some(img) = &ev.image {
                ev.image_exists = Some(docker.image_exists(img));
            }
        }
        ev.container = obs.container.clone();
        ev.published_ports = obs.inspect.as_ref().map(model::published_ports).unwrap_or_default();
        ev.memory_limit_mb = obs.inspect.as_ref().and_then(model::memory_limit_mb);
        ev.probe = obs.probe.clone();
        ev.stats = obs.stats.clone();
    }
    Ok((ev, obs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{probe, state};
    use std::collections::BTreeMap;

    fn spec() -> AppSpec {
        AppSpec {
            name: "shop".into(),
            path: "/srv/shop".into(),
            dockerfile: "Dockerfile".into(),
            port: Some(8080),
            host_port: Some(18200),
            health_path: "/healthz".into(),
            health_timeout_s: 60,
            cpu_millis: 500,
            ram_mb: 256,
            gpu_count: 0,
            vram_mb: 0,
            max_cost_cents_per_hour: u64::MAX,
            env: BTreeMap::new(),
            stack: vec![],
            sources: BTreeMap::new(),
        }
    }

    /// A healthy, running app; each test breaks one thing.
    fn base() -> Evidence {
        Evidence {
            app: "shop".into(),
            release: Some(2),
            target_id: "local".into(),
            container_name: "ltn-shop-r2".into(),
            container: Some(state(true, false, 0, false, 0)),
            published_ports: vec!["8080/tcp -> 0.0.0.0:18200".into()],
            memory_limit_mb: Some(256),
            logs: "server listening on 0.0.0.0:8080\nGET /healthz 200".into(),
            probe: Some(probe(true, Some(200), "ok", None)),
            stats: Some(Stats {
                cpu_percent: 0.4,
                memory_used_mb: 20.0,
                memory_limit_mb: 256.0,
                memory_percent: 7.8,
                pids: Some(4),
                raw_mem_usage: "20MiB / 256MiB".into(),
            }),
            spec: Some(spec()),
            image: Some("ltn/shop:r2".into()),
            image_exists: Some(true),
            previous_release: None,
            recent_audit: vec![],
            ..Default::default()
        }
    }

    fn stopped(exit: i64, oom: bool, logs: &str) -> Evidence {
        let mut e = base();
        e.container = Some(state(false, false, exit, oom, 0));
        e.probe = None;
        e.stats = None;
        e.logs = logs.into();
        e
    }

    fn has(f: &[Finding], needle: &str) -> Finding {
        f.iter()
            .find(|x| x.cause.contains(needle))
            .cloned()
            .unwrap_or_else(|| panic!("no finding containing {needle:?} in {f:#?}"))
    }

    fn quoted(f: &Finding, needle: &str) -> bool {
        f.evidence.iter().any(|e| e.contains(needle))
    }

    #[test]
    fn healthy_app_says_so_with_evidence() {
        let ev = base();
        let f = diagnose(&ev);
        assert_eq!(ev.verdict(), Verdict::Healthy);
        assert_eq!(f.len(), 1, "{f:#?}");
        assert_eq!(f[0].severity, Severity::Info);
        assert!(f[0].cause.contains("no problem found"));
        assert!(quoted(&f[0], "HTTP 200"));
        assert!(quoted(&f[0], "20MiB / 256MiB"));
        assert!(f[0].suggested_action.is_none());
    }

    #[test]
    fn oom_killed_suggests_raising_ram_and_quotes_limit() {
        let ev = stopped(137, true, "allocating buffer");
        let f = diagnose(&ev);
        let oom = has(&f, "OOMKilled");
        assert_eq!(oom.severity, Severity::Critical);
        assert!(quoted(&oom, "256 MB"));
        assert!(quoted(&oom, "OOMKilled=true"));
        let a = oom.suggested_action.unwrap();
        assert_eq!(a.tool, "ltn_plan");
        assert_eq!(a.arguments["path"], "/srv/shop");
        assert!(a.why.contains("ram_mb") && a.why.contains("512"));
        assert_eq!(ev.verdict(), Verdict::Down);
        assert!(!f.iter().any(|x| x.cause.contains("SIGKILL")));
    }

    #[test]
    fn exit_137_without_oom_is_external_kill() {
        let f = diagnose(&stopped(137, false, ""));
        let k = has(&f, "SIGKILL");
        assert!(quoted(&k, "ExitCode=137") && quoted(&k, "OOMKilled=false"));
        assert_eq!(k.suggested_action.unwrap().tool, "ltn_repair");
    }

    #[test]
    fn exit_126_and_127() {
        let f = diagnose(&stopped(126, false, "exec /app/start.sh: permission denied"));
        let x = has(&f, "exit 126");
        assert!(quoted(&x, "permission denied"));
        has(&f, "permission denied");
        let f = diagnose(&stopped(127, false, "/bin/sh: gunicorn: not found"));
        let x = has(&f, "not found (exit 127)");
        assert!(quoted(&x, "gunicorn: not found"));
        assert!(x.suggested_action.unwrap().why.contains("CMD"));
    }

    #[test]
    fn other_nonzero_exit_quotes_last_log_lines() {
        let f = diagnose(&stopped(3, false, "starting\nloading config\nfatal: DATABASE_URL is not set"));
        let x = has(&f, "exited with code 3");
        assert!(quoted(&x, "fatal: DATABASE_URL is not set"));
        assert!(quoted(&x, "ExitCode=3"));
        assert_eq!(x.suggested_action.unwrap().tool, "ltn_logs");
    }

    #[test]
    fn crash_loop_from_restarting_or_restart_count() {
        let mut ev = base();
        ev.container = Some(state(true, true, 1, false, 5));
        ev.logs = "boom".into();
        let f = diagnose(&ev);
        let x = has(&f, "crash-looping");
        assert!(quoted(&x, "RestartCount=5") && quoted(&x, "log: boom"));
        assert_eq!(ev.verdict(), Verdict::Degraded);

        let mut ev = base();
        ev.container = Some(state(true, false, 0, false, 3));
        has(&diagnose(&ev), "crash-looping");
    }

    #[test]
    fn connection_refused_on_declared_port() {
        let mut ev = base();
        ev.probe = Some(probe(false, None, "refused", Some("Connection Failed: Connect error: connection refused")));
        ev.logs = "Listening on http://127.0.0.1:3000".into();
        let f = diagnose(&ev);
        let x = has(&f, "not listening on that port");
        assert_eq!(x.severity, Severity::Critical);
        assert!(quoted(&x, "port = 8080, host_port = 18200"));
        assert!(quoted(&x, "8080/tcp -> 0.0.0.0:18200"));
        assert!(quoted(&x, "127.0.0.1:3000"));
        assert!(x.suggested_action.unwrap().why.contains("port"));
        // The probe's own refusal is not mistaken for a dependency problem.
        assert!(!f.iter().any(|x| x.cause.contains("dependency")));
    }

    #[test]
    fn http_5xx_and_4xx() {
        let mut ev = base();
        ev.probe = Some(probe(false, Some(502), "http_status", None));
        ev.logs = "upstream error: db timeout".into();
        let f = diagnose(&ev);
        let x = has(&f, "HTTP 502");
        assert!(quoted(&x, "db timeout"));
        assert_eq!(x.severity, Severity::Critical);

        let mut ev = base();
        ev.probe = Some(probe(false, Some(404), "http_status", None));
        let x = has(&diagnose(&ev), "returns HTTP 404");
        assert_eq!(x.severity, Severity::Warning);
        assert!(x.suggested_action.unwrap().why.contains("health_path"));
    }

    #[test]
    fn probe_timeout() {
        let mut ev = base();
        ev.probe = Some(probe(false, None, "timeout", Some("timed out reading response")));
        let x = has(&diagnose(&ev), "timed out");
        assert!(quoted(&x, "timeout after 3 ms"));
    }

    #[test]
    fn container_missing_and_image_missing() {
        let mut ev = base();
        ev.container = None;
        ev.probe = None;
        ev.stats = None;
        ev.logs = String::new();
        ev.image_exists = Some(false);
        let f = diagnose(&ev);
        let m = has(&f, "no container ltn-shop-r2");
        assert_eq!(m.severity, Severity::Critical);
        assert_eq!(m.suggested_action.unwrap().tool, "ltn_plan");
        let i = has(&f, "image ltn/shop:r2 no longer exists");
        assert_eq!(i.severity, Severity::Critical);
        assert_eq!(ev.verdict(), Verdict::Missing);
    }

    #[test]
    fn target_unreachable_stops_there() {
        let mut ev = base();
        ev.target_error = Some("Cannot connect to the Docker daemon at ssh://deploy@10.0.0.5".into());
        let f = diagnose(&ev);
        assert_eq!(f.len(), 1);
        assert!(f[0].cause.contains("unreachable"));
        assert!(quoted(&f[0], "Cannot connect to the Docker daemon"));
        assert_eq!(f[0].suggested_action.as_ref().unwrap().tool, "ltn_targets");
        assert_eq!(ev.verdict(), Verdict::Missing);
    }

    #[test]
    fn memory_pressure() {
        let mut ev = base();
        ev.stats = Some(Stats {
            cpu_percent: 3.0,
            memory_used_mb: 240.0,
            memory_limit_mb: 256.0,
            memory_percent: 93.75,
            pids: Some(4),
            raw_mem_usage: "240MiB / 256MiB".into(),
        });
        let x = has(&diagnose(&ev), "above 90%");
        assert!(quoted(&x, "240MiB / 256MiB"));
        assert!(x.suggested_action.unwrap().why.contains("ram_mb"));
    }

    #[test]
    fn log_patterns() {
        let cases = [
            ("thread 'main' panicked at src/main.rs:10:5:\ncalled unwrap on None", "panicked"),
            ("Traceback (most recent call last):\n  File \"app.py\", line 3\nKeyError: 'X'", "Python exception"),
            ("Error: listen EADDRINUSE: address already in use :::8080", "already in use"),
            ("open /data/db.sqlite: permission denied", "permission denied"),
            ("open /etc/app/config.yaml: no such file or directory", "does not exist in the image"),
            ("Error: connect ECONNREFUSED 10.0.0.9:5432", "dependency"),
            ("java.lang.OutOfMemoryError: Java heap space", "out of memory"),
        ];
        for (logs, cause) in cases {
            let mut ev = base();
            ev.logs = logs.into();
            let f = diagnose(&ev);
            let x = has(&f, cause);
            let first = logs.lines().next().unwrap();
            assert!(quoted(&x, first.trim()), "{cause}: {x:#?}");
        }
    }

    #[test]
    fn failing_release_with_previous_suggests_rollback() {
        let mut ev = stopped(1, false, "crash");
        ev.previous_release = Some(1);
        let f = diagnose(&ev);
        let r = f
            .iter()
            .find(|x| x.suggested_action.as_ref().map_or(false, |a| a.tool == "ltn_rollback"))
            .expect("rollback suggestion");
        let a = r.suggested_action.clone().unwrap();
        assert_eq!(a.arguments, json!({ "app": "shop", "to_release": 1 }));
        assert!(r.cause.contains("r2") && r.cause.contains("r1"));
        // Healthy app with a previous release: no rollback suggested.
        let mut ok = base();
        ok.previous_release = Some(1);
        assert!(!diagnose(&ok)
            .iter()
            .any(|x| x.suggested_action.as_ref().map_or(false, |a| a.tool == "ltn_rollback")));
    }

    #[test]
    fn crash_loop_with_previous_release_points_at_it() {
        let mut ev = base();
        ev.container = Some(state(true, true, 2, false, 4));
        ev.previous_release = Some(1);
        let x = has(&diagnose(&ev), "crash-looping");
        assert_eq!(x.suggested_action.unwrap().tool, "ltn_rollback");
    }

    #[test]
    fn findings_are_ordered_critical_first() {
        let mut ev = stopped(1, false, "Error: connect ECONNREFUSED 10.0.0.9:5432");
        ev.recent_audit = vec![json!({"at": "2026-11-20T00:00:00Z", "action": "watch.start", "app": "shop"})];
        let f = diagnose(&ev);
        assert_eq!(f[0].severity, Severity::Critical);
        assert_eq!(f.last().unwrap().severity, Severity::Info);
        has(&f, "watcher or ltn_repair acted");
    }
}
