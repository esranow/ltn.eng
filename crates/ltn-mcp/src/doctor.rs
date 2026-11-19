//! `ltn_doctor`: checks the things the other tools depend on, for real.

use crate::{find_binary, find_on_path};
use anyhow::Result;
use ltn_state::store::Target;
use ltn_state::{place, Ctx, PlacementRequest, TargetCapacity, ToolDef};
use serde_json::{json, Value};

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

pub fn run(ctx: &Ctx, _args: &Value) -> Result<Value> {
    let mut checks = Vec::new();

    // Docker on the local target, through the configured binary.
    let docker_ok = match ctx.docker(&Target::local()).info() {
        Ok(i) => {
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
        Err(e) => {
            checks.push(check(
                "docker",
                false,
                format!("local Docker daemon not usable via `{}`: {e:#}", ctx.docker_bin),
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

    // Provisioners and their offers.
    let mut offers = 0usize;
    let mut offer_errors = Vec::new();
    for p in &ctx.provisioners {
        match p.offers(ctx) {
            Ok(o) => offers += o.len(),
            Err(e) => offer_errors.push(format!("{}: {e:#}", p.provider())),
        }
    }
    let providers: Vec<&str> = ctx.provisioners.iter().map(|p| p.provider()).collect();
    checks.push(check(
        "provisioners",
        offer_errors.is_empty(),
        format!(
            "{} provisioner(s) [{}], {} offer(s) available{}",
            providers.len(),
            providers.join(", "),
            offers,
            if offer_errors.is_empty() { String::new() } else { format!("; errors: {}", offer_errors.join("; ")) }
        ),
    ));

    // Recorded state.
    let (targets, deployments) = match ctx.store().load() {
        Ok(st) => {
            let (t, d) = (st.all_targets().len(), st.deployments.len());
            checks.push(check("state", true, format!("{t} target(s), {d} deployment(s) recorded")));
            (Some(t), Some(d))
        }
        Err(e) => {
            checks.push(check("state", false, format!("could not read state: {e:#}")));
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
