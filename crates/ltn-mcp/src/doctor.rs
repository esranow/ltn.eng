//! `ltn_doctor`: checks the things the other tools depend on, for real.

use crate::{find_binary, find_on_path};
use anyhow::Result;
use ltn_state::store::Target;
use ltn_state::{place, Ctx, PlacementRequest, TargetCapacity, ToolDef};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn tool() -> ToolDef {
    ToolDef {
        name: "ltn_doctor",
        description: "Check the server's environment: Docker daemon reachable (version, CPUs, memory), \
                      Z3 solver linked, state directory writable, ssh/OpenTofu/map-mtl binaries found, \
                      provisioner offers, and how many targets and deployments are recorded. Overall \
                      `ok` is true only when Docker, Z3 and the state directory pass. Run this first.",
        input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        handler: run,
    }
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> Value {
    json!({ "name": name, "ok": ok, "detail": detail.into() })
}

/// How long one external check (Docker, provisioner offers, state read) may take.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the whole doctor run may take.
pub const OVERALL_TIMEOUT: Duration = Duration::from_secs(25);

pub fn run(ctx: &Ctx, _args: &Value) -> Result<Value> {
    run_bounded(ctx, CHECK_TIMEOUT, OVERALL_TIMEOUT)
}

/// Start `f` on its own thread and hand back a receiver for its result.
/// A check that overruns is abandoned, not joined: its thread finishes (or
/// the Docker call hits its own time limit) in the background.
fn spawn_check<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> mpsc::Receiver<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx
}

/// Wait for a check until its own bound or the overall deadline, whichever
/// comes first. `Err(limit)` means it did not answer in time.
fn wait<T>(rx: &mpsc::Receiver<T>, started: Instant, per_check: Duration, overall_deadline: Instant) -> Result<T, Duration> {
    let deadline = (started + per_check).min(overall_deadline);
    let left = deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(left).map_err(|_| deadline.saturating_duration_since(started))
}

fn secs(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64())
}

/// `run` with explicit bounds, so tests can use short ones.
pub fn run_bounded(ctx: &Ctx, per_check: Duration, overall: Duration) -> Result<Value> {
    let started = Instant::now();
    let overall_deadline = started + overall;

    // Independent external checks, started together.
    let docker = ctx.docker(&Target::local());
    let docker_rx = spawn_check(move || docker.info());
    let pctx = ctx.clone();
    let offers_rx = spawn_check(move || {
        let mut offers = 0usize;
        let mut errors = Vec::new();
        for p in &pctx.provisioners {
            match p.offers(&pctx) {
                Ok(o) => offers += o.len(),
                Err(e) => errors.push(format!("{}: {e:#}", p.provider())),
            }
        }
        (offers, errors)
    });
    let sctx = ctx.clone();
    let state_rx = spawn_check(move || sctx.store().load().map(|st| (st.all_targets().len(), st.deployments.len())));

    let mut checks = Vec::new();

    // Docker on the local target, through the configured binary.
    let docker_ok = match wait(&docker_rx, started, per_check, overall_deadline) {
        Ok(Ok(i)) => {
            checks.push(check(
                "docker",
                true,
                format!(
                    "Docker {} on {} via `{}`: {} CPUs, {} MB memory{}",
                    i.server_version,
                    i.os,
                    ctx.docker_bin,
                    i.cpus,
                    i.memory_mb,
                    if i.has_nvidia_runtime { ", nvidia runtime present" } else { "" }
                ),
            ));
            true
        }
        Ok(Err(e)) => {
            checks.push(check(
                "docker",
                false,
                format!("local Docker daemon not usable via `{}`: {e:#}", ctx.docker_bin),
            ));
            false
        }
        Err(limit) => {
            checks.push(check(
                "docker",
                false,
                format!(
                    "Docker did not answer within {} s via `{}` (`docker info`); the daemon is overloaded or stopped",
                    secs(limit),
                    ctx.docker_bin
                ),
            ));
            false
        }
    };

    // Z3: solve a trivial placement and confirm the answer.
    let request = PlacementRequest { cpu_millis: 100, ram_mb: 64, gpu_count: 0, vram_mb: 0, max_cost_cents_per_hour: 0 };
    let caps = [TargetCapacity {
        target_id: "doctor-probe".into(),
        free_cpu_millis: 1000,
        free_ram_mb: 1024,
        free_gpu_count: 0,
        free_vram_mb: 0,
        cost_cents_per_hour: 0,
    }];
    let z3_ok = match place(&request, &caps) {
        Ok(d) if d.target_id == "doctor-probe" => {
            checks.push(check("z3", true, format!("Z3 solved a probe placement ({} bytes of SMT-LIB2)", d.smt2.len())));
            true
        }
        Ok(d) => {
            checks.push(check("z3", false, format!("Z3 returned an unexpected target `{}` for the probe", d.target_id)));
            false
        }
        Err(e) => {
            checks.push(check("z3", false, format!("Z3 probe placement failed: {e:?}")));
            false
        }
    };

    // State directory: create it and write/remove a file.
    let state_ok = match state_dir_writable(ctx) {
        Ok(()) => {
            checks.push(check("state_dir", true, format!("{} is writable", ctx.home.display())));
            true
        }
        Err(e) => {
            checks.push(check("state_dir", false, format!("{} is not writable: {e}", ctx.home.display())));
            false
        }
    };

    match find_on_path("ssh") {
        Some(p) => checks.push(check("ssh", true, format!("found {}", p.display()))),
        None => checks.push(check("ssh", false, "`ssh` not found on PATH; SSH docker hosts (ssh://...) will not work")),
    }
    match find_binary("LTN_TOFU_BIN", "tofu") {
        Ok(p) => checks.push(check("opentofu", true, format!("found {}", p.display()))),
        Err(e) => checks.push(check("opentofu", false, format!("{e}; cloud provisioning will fail"))),
    }
    match find_binary("LTN_MAP_MTL_BIN", "map-mtl") {
        Ok(p) => checks.push(check("map_mtl", true, format!("found {}", p.display()))),
        Err(e) => checks.push(check("map_mtl", false, format!("{e}; map_mtl_topology falls back to the built-in probe"))),
    }

    // Provisioners and their offers (may call the cloud CLI).
    let providers: Vec<&str> = ctx.provisioners.iter().map(|p| p.provider()).collect();
    let offers = match wait(&offers_rx, started, per_check, overall_deadline) {
        Ok((offers, errors)) => {
            checks.push(check(
                "provisioners",
                errors.is_empty(),
                format!(
                    "{} provisioner(s) [{}], {} offer(s) available{}",
                    providers.len(),
                    providers.join(", "),
                    offers,
                    if errors.is_empty() { String::new() } else { format!("; errors: {}", errors.join("; ")) }
                ),
            ));
            offers
        }
        Err(limit) => {
            checks.push(check(
                "provisioners",
                false,
                format!(
                    "{} provisioner(s) [{}] did not list offers within {} s (cloud CLI slow or hanging)",
                    providers.len(),
                    providers.join(", "),
                    secs(limit)
                ),
            ));
            0
        }
    };

    // Recorded state.
    let (targets, deployments) = match wait(&state_rx, started, per_check, overall_deadline) {
        Ok(Ok((t, d))) => {
            checks.push(check("state", true, format!("{t} target(s), {d} deployment(s) recorded")));
            (Some(t), Some(d))
        }
        Ok(Err(e)) => {
            checks.push(check("state", false, format!("could not read state: {e:#}")));
            (None, None)
        }
        Err(limit) => {
            checks.push(check(
                "state",
                false,
                format!("could not read state within {} s (state file locked by another process?)", secs(limit)),
            ));
            (None, None)
        }
    };

    Ok(json!({
        "ok": docker_ok && z3_ok && state_ok,
        "checks": checks,
        "provisioners": providers.len(),
        "offers": offers,
        "targets": targets,
        "deployments": deployments,
        "state_dir": ctx.home.display().to_string(),
        "docker_bin": ctx.docker_bin,
        "elapsed_s": started.elapsed().as_secs_f64(),
    }))
}

fn state_dir_writable(ctx: &Ctx) -> std::io::Result<()> {
    std::fs::create_dir_all(&ctx.home)?;
    let probe = ctx.home.join(format!(".doctor-{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_docker_binary_fails_overall_and_explains() {
        let home = std::env::temp_dir().join(format!("ltn-doctor-{}", uuid::Uuid::new_v4().simple()));
        let mut ctx = Ctx::for_home(&home);
        ctx.docker_bin = home.join("no-such-docker.exe").display().to_string();
        let out = run(&ctx, &json!({})).unwrap();
        assert_eq!(out["ok"], false);
        let checks = out["checks"].as_array().unwrap();
        let docker = checks.iter().find(|c| c["name"] == "docker").unwrap();
        assert_eq!(docker["ok"], false);
        let detail = docker["detail"].as_str().unwrap();
        assert!(detail.contains("no-such-docker"), "{detail}");
        assert!(detail.contains("could not run"), "{detail}");
        // The other core checks still ran for real.
        let z3 = checks.iter().find(|c| c["name"] == "z3").unwrap();
        assert_eq!(z3["ok"], true, "{z3}");
        let sd = checks.iter().find(|c| c["name"] == "state_dir").unwrap();
        assert_eq!(sd["ok"], true, "{sd}");
        assert_eq!(out["targets"], 1);
        assert_eq!(out["deployments"], 0);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A stand-in `docker` that ignores its arguments and sleeps.
    fn sleeping_docker(dir: &std::path::Path) -> String {
        std::fs::create_dir_all(dir).unwrap();
        #[cfg(windows)]
        {
            let p = dir.join("slow-docker.cmd");
            std::fs::write(&p, "@ping -n 30 127.0.0.1 >nul\r\n").unwrap();
            p.display().to_string()
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.join("slow-docker");
            std::fs::write(&p, "#!/bin/sh\nsleep 30\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p.display().to_string()
        }
    }

    #[test]
    fn stalled_docker_is_reported_within_the_bound() {
        let home = std::env::temp_dir().join(format!("ltn-doctor-{}", uuid::Uuid::new_v4().simple()));
        let mut ctx = Ctx::for_home(&home);
        ctx.docker_bin = sleeping_docker(&home.join("bin"));
        let start = Instant::now();
        let out = run_bounded(&ctx, Duration::from_secs(2), Duration::from_secs(4)).unwrap();
        let took = start.elapsed();
        assert!(took < Duration::from_secs(5), "doctor took {took:?}");
        assert_eq!(out["ok"], false);
        let checks = out["checks"].as_array().unwrap();
        let docker = checks.iter().find(|c| c["name"] == "docker").unwrap();
        assert_eq!(docker["ok"], false);
        let detail = docker["detail"].as_str().unwrap();
        assert!(detail.contains("did not answer within 2.0 s"), "{detail}");
        // The rest of the report is still complete and real.
        let names: Vec<&str> = checks.iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["docker", "z3", "state_dir", "ssh", "opentofu", "map_mtl", "provisioners", "state"]);
        assert_eq!(checks.iter().find(|c| c["name"] == "z3").unwrap()["ok"], true);
        assert_eq!(out["targets"], 1);
    }

    #[test]
    #[ignore = "requires a running Docker daemon"]
    fn real_docker_passes() {
        let home = std::env::temp_dir().join(format!("ltn-doctor-{}", uuid::Uuid::new_v4().simple()));
        let ctx = Ctx::for_home(&home);
        let out = run(&ctx, &json!({})).unwrap();
        assert_eq!(out["ok"], true, "{out:#}");
        let _ = std::fs::remove_dir_all(&home);
    }
}
