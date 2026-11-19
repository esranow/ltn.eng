//! Real-Docker tests of the engine tools. Run with
//! `cargo test -p ltn-engine -- --ignored --test-threads=1`.
//! Host ports used: 18101-18104.

use ltn_state::Ctx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tool(ctx: &Ctx, name: &str, args: Value) -> anyhow::Result<Value> {
    let t = ltn_engine::tools().into_iter().find(|t| t.name == name).expect(name);
    (t.handler)(ctx, &args)
}

fn tmp(prefix: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app_name(prefix: &str) -> String {
    format!("{prefix}-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

/// Removes every container and image of the app, even when the test panics.
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let out = Command::new("docker")
            .args(["ps", "-aq", "--filter", &format!("label=ltn.app={}", self.0)])
            .output();
        if let Ok(o) = out {
            for id in String::from_utf8_lossy(&o.stdout).split_whitespace() {
                let _ = Command::new("docker").args(["rm", "-f", id]).output();
            }
        }
        let out = Command::new("docker")
            .args(["images", &format!("ltn/{}", self.0), "--format", "{{.Repository}}:{{.Tag}}"])
            .output();
        if let Ok(o) = out {
            for img in String::from_utf8_lossy(&o.stdout).split_whitespace() {
                let _ = Command::new("docker").args(["rmi", "-f", img]).output();
            }
        }
    }
}

const HEALTHY: &str = "FROM busybox\nRUN mkdir /www && echo {TEXT} > /www/index.html\nEXPOSE 8080\nCMD [\"httpd\",\"-f\",\"-p\",\"8080\",\"-h\",\"/www\"]\n";
const CRASHING: &str = "FROM busybox\nEXPOSE 8080\nCMD [\"sh\",\"-c\",\"echo boom; exit 3\"]\n";
const NEVER_HEALTHY: &str = "FROM busybox\nEXPOSE 8080\nCMD [\"sleep\",\"3600\"]\n";

fn write_project(dir: &Path, app: &str, port: u16, dockerfile: &str) {
    std::fs::write(dir.join("Dockerfile"), dockerfile).unwrap();
    std::fs::write(
        dir.join("ltn.toml"),
        format!("name = \"{app}\"\nhost_port = {port}\nhealth_timeout_s = 8\ncpu = 0.25\nram_mb = 64\n"),
    )
    .unwrap();
}

fn healthy(text: &str) -> String {
    HEALTHY.replace("{TEXT}", text)
}

fn get(port: u16) -> Result<String, String> {
    ureq::get(&format!("http://127.0.0.1:{port}/"))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

fn plan_apply(ctx: &Ctx, dir: &Path) -> anyhow::Result<Value> {
    let plan = tool(ctx, "ltn_plan", json!({ "path": dir }))?;
    tool(ctx, "ltn_apply", json!({ "plan_id": plan["plan_id"] }))
}

fn container_exists(name: &str) -> bool {
    Command::new("docker")
        .args(["inspect", "--type", "container", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn full_lifecycle_apply_replace_fail_restore_rollback_destroy() {
    const PORT: u16 = 18101;
    let app = app_name("e1life");
    let _cleanup = Cleanup(app.clone());
    let ctx = Ctx::for_home(&tmp("ltn-engine-it"));
    let dir = tmp("ltn-engine-fixture");

    // v1: plan (Z3) + apply, page served.
    write_project(&dir, &app, PORT, &healthy("v1"));
    let plan = tool(&ctx, "ltn_plan", json!({ "path": dir })).unwrap();
    assert_eq!(plan["target_id"], "local");
    assert!(plan["provision_offer"].is_null());
    assert!(plan["capacity_considered"].as_array().unwrap().len() >= 1);
    let r1 = tool(&ctx, "ltn_apply", json!({ "plan_id": plan["plan_id"] })).unwrap();
    assert_eq!(r1["release"], 1);
    assert_eq!(r1["health"]["healthy"], true);
    assert_eq!(r1["endpoint"], format!("http://127.0.0.1:{PORT}"));
    assert_eq!(get(PORT).unwrap().trim(), "v1");
    // The same plan cannot be applied twice.
    assert!(tool(&ctx, "ltn_apply", json!({ "plan_id": plan["plan_id"] })).is_err());

    // v2 replaces v1: new content, old container gone, v1 image kept.
    write_project(&dir, &app, PORT, &healthy("v2"));
    let r2 = plan_apply(&ctx, &dir).unwrap();
    assert_eq!(r2["release"], 2);
    assert_eq!(r2["from_release"], 1);
    assert_eq!(get(PORT).unwrap().trim(), "v2");
    assert!(!container_exists(&format!("ltn-{app}-r1")));
    assert!(container_exists(&format!("ltn-{app}-r2")));

    // A crashing release fails with exit code and logs, and v2 comes back.
    write_project(&dir, &app, PORT, CRASHING);
    let err = plan_apply(&ctx, &dir).unwrap_err().to_string();
    eprintln!("--- apply error ---
{err}");
    assert!(err.contains("exit_code=3") || err.contains("exit code 3"), "{err}");
    assert!(err.contains("boom"), "{err}");
    assert!(err.contains("running again"), "{err}");
    assert_eq!(get(PORT).unwrap().trim(), "v2");
    assert!(!container_exists(&format!("ltn-{app}-r3")));

    // A release that never answers fails by timeout and v2 is restored.
    write_project(&dir, &app, PORT, NEVER_HEALTHY);
    let err = plan_apply(&ctx, &dir).unwrap_err().to_string();
    eprintln!("--- apply error ---
{err}");
    assert!(err.contains("timed out after 8s"), "{err}");
    assert!(err.contains("running again"), "{err}");
    assert_eq!(get(PORT).unwrap().trim(), "v2");

    let st = ctx.store().load().unwrap();
    let d = &st.deployments[&app];
    assert_eq!(d.current, Some(2));
    let states: Vec<(u32, String)> = d.releases.iter().map(|r| (r.number, r.state.clone())).collect();
    assert_eq!(
        states,
        vec![(1, "superseded".into()), (2, "active".into()), (3, "failed".into()), (4, "failed".into())]
    );
    assert!(!ltn_engine::is_locked(&st, &app, chrono::Utc::now()));

    // Rollback (default target = latest superseded) returns to v1.
    let rb = tool(&ctx, "ltn_rollback", json!({ "app": app })).unwrap();
    assert_eq!(rb["release"], 1);
    assert_eq!(rb["from_release"], 2);
    assert_eq!(rb["health"]["healthy"], true);
    assert_eq!(get(PORT).unwrap().trim(), "v1");
    assert!(!container_exists(&format!("ltn-{app}-r2")));
    let st = ctx.store().load().unwrap();
    let d = &st.deployments[&app];
    assert_eq!(d.current, Some(1));
    assert_eq!(d.releases.iter().find(|r| r.number == 2).unwrap().state, "rolled_back");

    // Destroy needs the confirm and removes everything.
    assert!(tool(&ctx, "ltn_destroy", json!({ "app": app, "confirm": "nope" })).is_err());
    let out = tool(&ctx, "ltn_destroy", json!({ "app": app, "confirm": app })).unwrap();
    assert_eq!(out["removed_containers"].as_array().unwrap().len(), 1);
    assert!(!container_exists(&format!("ltn-{app}-r1")));
    assert!(get(PORT).is_err());
    assert!(!ctx.store().load().unwrap().deployments.contains_key(&app));
    let imgs = Command::new("docker").args(["images", "-q", &format!("ltn/{app}")]).output().unwrap();
    assert!(String::from_utf8_lossy(&imgs.stdout).trim().is_empty());
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn first_deploy_that_crashes_leaves_nothing_running() {
    const PORT: u16 = 18103;
    let app = app_name("e1crash");
    let _cleanup = Cleanup(app.clone());
    let ctx = Ctx::for_home(&tmp("ltn-engine-it"));
    let dir = tmp("ltn-engine-fixture");
    write_project(&dir, &app, PORT, CRASHING);
    let err = plan_apply(&ctx, &dir).unwrap_err().to_string();
    eprintln!("--- apply error ---
{err}");
    assert!(err.contains("boom") && err.contains("no running previous release"), "{err}");
    assert!(!container_exists(&format!("ltn-{app}-r1")));
    assert!(ctx.store().load().unwrap().deployments.is_empty());
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn rollback_to_a_pruned_or_missing_image_is_an_error() {
    const PORT: u16 = 18104;
    let app = app_name("e1prune");
    let _cleanup = Cleanup(app.clone());
    let ctx = Ctx::for_home(&tmp("ltn-engine-it"));
    let dir = tmp("ltn-engine-fixture");
    write_project(&dir, &app, PORT, &healthy("a"));
    plan_apply(&ctx, &dir).unwrap();
    write_project(&dir, &app, PORT, &healthy("b"));
    plan_apply(&ctx, &dir).unwrap();
    let _ = Command::new("docker").args(["rmi", "-f", &format!("ltn/{app}:r1")]).output();
    let err = tool(&ctx, "ltn_rollback", json!({ "app": app })).unwrap_err().to_string();
    assert!(err.contains("no longer exists"), "{err}");
    assert_eq!(get(PORT).unwrap().trim(), "b");
    tool(&ctx, "ltn_destroy", json!({ "app": app, "confirm": app })).unwrap();
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn missing_docker_binary_makes_plan_and_apply_fail() {
    const PORT: u16 = 18102;
    let app = app_name("e1nodock");
    let _cleanup = Cleanup(app.clone());
    let home = tmp("ltn-engine-it");
    let dir = tmp("ltn-engine-fixture");
    write_project(&dir, &app, PORT, &healthy("x"));

    let real = Ctx::for_home(&home);
    let plan = tool(&real, "ltn_plan", json!({ "path": dir })).unwrap();

    let mut broken = Ctx::for_home(&home);
    broken.docker_bin = "ltn-docker-does-not-exist".into();
    assert!(tool(&broken, "ltn_plan", json!({ "path": dir })).is_err());
    let err = tool(&broken, "ltn_apply", json!({ "plan_id": plan["plan_id"] })).unwrap_err();
    assert!(format!("{err:#}").contains("could not run"), "{err:#}");
    let st = real.store().load().unwrap();
    assert!(st.deployments.is_empty());
    assert!(!container_exists(&format!("ltn-{app}-r1")));
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn target_add_with_unreachable_ssh_host_saves_nothing() {
    let ctx = Ctx::for_home(&tmp("ltn-engine-it"));
    let err = tool(
        &ctx,
        "ltn_target_add",
        json!({ "id": "nowhere", "docker_host": "ssh://nobody@127.0.0.1:1", "address": "127.0.0.1" }),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("not added"), "{err}");
    assert!(ctx.store().load().unwrap().targets.is_empty());
    let list = tool(&ctx, "ltn_targets", json!({})).unwrap();
    let targets = list["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0]["id"], "local");
    assert_eq!(targets[0]["reachable"], true);
    assert!(targets[0]["capacity"]["free"]["free_cpu_millis"].as_u64().unwrap() > 0);
}
