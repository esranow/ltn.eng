//! Unit tests of pure logic and guards. None of these need Docker: where a
//! handler is exercised, Docker is deliberately pointed at a missing binary.

use crate::capacity::{budget_cap, free_capacity, reserved_on, Reservation};
use crate::deploy::{self, rollback_target, run_args, BusyLock};
use crate::plan::{self, check_plan_usable, host_port_conflict, plan_with_env_cap, solve};
use crate::targets;
use chrono::{Duration, Utc};
use ltn_state::spec::AppSpec;
use ltn_state::store::{Deployment, Plan, Release, State, Target};
use ltn_state::{Ctx, Offer, PlacementDecision, Provisioner, TargetCapacity};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn tmp(prefix: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Ctx whose Docker binary does not exist, so any docker call fails.
fn no_docker_ctx() -> Ctx {
    let mut ctx = Ctx::for_home(&tmp("ltn-engine-home"));
    ctx.docker_bin = "ltn-no-such-docker-binary".into();
    ctx
}

fn project(toml: &str) -> PathBuf {
    let d = tmp("ltn-engine-proj");
    std::fs::write(d.join("Dockerfile"), "FROM busybox\nEXPOSE 8080\n").unwrap();
    std::fs::write(d.join("ltn.toml"), toml).unwrap();
    d
}

fn spec(name: &str, cpu: u64, ram: u64, host_port: Option<u16>) -> AppSpec {
    AppSpec {
        name: name.into(),
        path: ".".into(),
        dockerfile: "Dockerfile".into(),
        port: host_port.map(|_| 8080),
        host_port,
        health_path: "/".into(),
        health_timeout_s: 10,
        cpu_millis: cpu,
        ram_mb: ram,
        gpu_count: 0,
        vram_mb: 0,
        max_cost_cents_per_hour: u64::MAX,
        env: BTreeMap::new(),
        stack: vec![],
        sources: BTreeMap::new(),
    }
}

fn deployment(app: &str, target: &str, cpu: u64, ram: u64, host_port: Option<u16>, current: Option<u32>) -> Deployment {
    Deployment {
        app: app.into(),
        target_id: target.into(),
        spec: spec(app, cpu, ram, host_port),
        releases: current
            .map(|n| {
                vec![Release { number: n, image: format!("ltn/{app}:r{n}"), plan_id: None, created_at: Utc::now(), state: "active".into() }]
            })
            .unwrap_or_default(),
        current,
        health_failures: 0,
    }
}

fn cap(id: &str, cpu: u64, ram: u64, cost: u64) -> TargetCapacity {
    TargetCapacity { target_id: id.into(), free_cpu_millis: cpu, free_ram_mb: ram, free_gpu_count: 0, free_vram_mb: 0, cost_cents_per_hour: cost }
}

struct FakeProvisioner {
    offers: Vec<Offer>,
    deprovisioned: Arc<Mutex<Vec<String>>>,
}

impl Provisioner for FakeProvisioner {
    fn provider(&self) -> &'static str {
        "fake"
    }
    fn offers(&self, _ctx: &Ctx) -> anyhow::Result<Vec<Offer>> {
        Ok(self.offers.clone())
    }
    fn provision(&self, _ctx: &Ctx, _offer_id: &str, _name: &str) -> anyhow::Result<Target> {
        anyhow::bail!("fake provisioner does not create machines")
    }
    fn deprovision(&self, _ctx: &Ctx, target: &Target) -> anyhow::Result<()> {
        self.deprovisioned.lock().unwrap().push(target.id.clone());
        Ok(())
    }
}

fn offer(id: &str, cpu: u64, ram: u64, cost: u64) -> Offer {
    Offer { offer_id: id.into(), provider: "fake".into(), description: "test".into(), capacity: cap(id, cpu, ram, cost) }
}

fn fake(ctx: &mut Ctx, offers: Vec<Offer>) -> Arc<Mutex<Vec<String>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    ctx.provisioners.push(Arc::new(FakeProvisioner { offers, deprovisioned: log.clone() }));
    log
}

fn stored_plan(ctx: &Ctx, applied: bool, age_min: i64) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let p = Plan {
        id: id.clone(),
        created_at: Utc::now() - Duration::minutes(age_min),
        spec: spec("planned-app", 100, 64, None),
        target_id: "local".into(),
        provision_offer: None,
        decision: PlacementDecision { target_id: "local".into(), cost_cents_per_hour: 0, ram_mb_left_free: 0, smt2: String::new() },
        capacity_considered: vec![],
        applied,
    };
    ctx.store().update(|st| {
        st.plans.insert(id.clone(), p);
        Ok(())
    })
    .unwrap();
    id
}

// ------------------------------------------------------------ capacity

#[test]
fn reservations_sum_current_releases_on_the_target_only() {
    let mut st = State::default();
    st.deployments.insert("a".into(), deployment("a", "local", 500, 512, None, Some(1)));
    st.deployments.insert("b".into(), deployment("b", "local", 1500, 1024, None, Some(3)));
    st.deployments.insert("c".into(), deployment("c", "edge", 4000, 4096, None, Some(1)));
    st.deployments.insert("d".into(), deployment("d", "local", 9000, 9000, None, None)); // nothing running
    let r = reserved_on(&st, "local", None);
    assert_eq!(r, Reservation { cpu_millis: 2000, ram_mb: 1536, gpu_count: 0, vram_mb: 0 });

    let total = cap("local", 8000, 8000, 0);
    let free = free_capacity(&total, r);
    assert_eq!((free.free_cpu_millis, free.free_ram_mb), (6000, 6464));

    // Re-planning `b` adds its reservation back: the new release replaces it.
    let free_b = free_capacity(&total, reserved_on(&st, "local", Some("b")));
    assert_eq!((free_b.free_cpu_millis, free_b.free_ram_mb), (7500, 7488));
}

#[test]
fn free_capacity_never_underflows() {
    let r = Reservation { cpu_millis: 10_000, ram_mb: 99_999, gpu_count: 2, vram_mb: 1 };
    let f = free_capacity(&cap("x", 1000, 1000, 7), r);
    assert_eq!((f.free_cpu_millis, f.free_ram_mb, f.free_gpu_count, f.cost_cents_per_hour), (0, 0, 0, 7));
}

#[test]
fn budget_is_min_of_spec_and_env_cap() {
    assert_eq!(budget_cap(u64::MAX, None).unwrap(), u64::MAX);
    assert_eq!(budget_cap(u64::MAX, Some("25")).unwrap(), 25);
    assert_eq!(budget_cap(10, Some("25")).unwrap(), 10);
    assert_eq!(budget_cap(10, Some("  ")).unwrap(), 10);
    assert!(budget_cap(10, Some("ten")).unwrap_err().to_string().contains("LTN_MAX_HOURLY_CENTS"));
}

// ------------------------------------------------------------ placement

#[test]
fn offer_wins_when_no_existing_target_fits() {
    let s = spec("big", 4000, 4096, None);
    let (d, o, considered) = solve(&s, &[cap("local", 2000, 2048, 0)], &[offer("fake:m@x", 8000, 16000, 10)], u64::MAX).unwrap();
    assert_eq!(d.target_id, "fake:m@x");
    assert_eq!(o.unwrap().offer_id, "fake:m@x");
    assert_eq!(considered.len(), 2);

    // A free existing target that fits beats a paid offer.
    let (d, o, _) = solve(&s, &[cap("local", 8000, 8000, 0)], &[offer("fake:m@x", 8000, 16000, 10)], u64::MAX).unwrap();
    assert_eq!(d.target_id, "local");
    assert!(o.is_none());
}

#[test]
fn unsat_error_names_core_and_each_candidates_capacity() {
    let s = spec("big", 500, 4096, None);
    let err = solve(&s, &[cap("local", 8000, 1024, 0)], &[], u64::MAX).unwrap_err().to_string();
    assert!(err.contains("min_ram"), "{err}");
    assert!(err.contains("local: free cpu_millis=8000 ram_mb=1024"), "{err}");
    assert!(err.contains("ram_mb=4096"), "{err}");
}

#[test]
fn plan_chooses_offer_when_docker_is_unreachable_and_lists_the_error() {
    let mut ctx = no_docker_ctx();
    fake(&mut ctx, vec![offer("fake:small@r1", 2000, 4096, 10)]);
    let dir = project("host_port = 18190\n");
    let out = plan_with_env_cap(&ctx, &json!({ "path": dir }), None).unwrap();
    assert_eq!(out["target_id"], "fake:small@r1");
    assert_eq!(out["provision_offer"], "fake:small@r1");
    assert_eq!(out["cost_cents_per_hour"], 10);
    assert_eq!(out["unreachable_targets"][0]["id"], "local");
    assert!(out["unreachable_targets"][0]["error"].as_str().unwrap().contains("could not run"));
    assert!(out["expires_at"].is_string());
    let plan_id = out["plan_id"].as_str().unwrap();
    let st = ctx.store().load().unwrap();
    assert_eq!(st.plans[plan_id].provision_offer.as_deref(), Some("fake:small@r1"));
}

#[test]
fn env_budget_below_offer_price_skips_the_offer() {
    let mut ctx = no_docker_ctx();
    fake(&mut ctx, vec![offer("fake:small@r1", 2000, 4096, 10)]);
    let dir = project("host_port = 18191\n");
    let err = plan_with_env_cap(&ctx, &json!({ "path": dir }), Some("5")).unwrap_err().to_string();
    assert!(err.contains("budget"), "{err}");
    assert!(err.contains("max_cost_cents_per_hour=5"), "{err}");
    assert!(ctx.store().load().unwrap().plans.is_empty());
}

#[test]
fn plan_without_docker_or_offers_is_an_error() {
    let ctx = no_docker_ctx();
    let dir = project("host_port = 18192\n");
    let err = plan_with_env_cap(&ctx, &json!({ "path": dir }), None).unwrap_err().to_string();
    assert!(err.contains("no usable capacity") && err.contains("could not run"), "{err}");
}

#[test]
fn host_port_used_by_another_app_on_the_target_is_rejected() {
    let mut st = State::default();
    st.deployments.insert("other".into(), deployment("other", "local", 100, 64, Some(18193), Some(1)));
    assert_eq!(host_port_conflict(&st, "local", "mine", Some(18193)).as_deref(), Some("other"));
    assert_eq!(host_port_conflict(&st, "local", "other", Some(18193)), None); // its own port
    assert_eq!(host_port_conflict(&st, "edge", "mine", Some(18193)), None);
    assert_eq!(host_port_conflict(&st, "local", "mine", Some(18194)), None);

    let ctx = no_docker_ctx();
    ctx.store().save(&st).unwrap();
    let dir = project("name = \"mine\"\nhost_port = 18193\n");
    let err = plan_with_env_cap(&ctx, &json!({ "path": dir, "target": "local" }), None).unwrap_err().to_string();
    assert!(err.contains("host_port 18193") && err.contains("`other`"), "{err}");
}

// ------------------------------------------------------------ plan guards

#[test]
fn plan_expiry_and_reuse_are_rejected() {
    let ctx = no_docker_ctx();
    let fresh = stored_plan(&ctx, false, 1);
    let applied = stored_plan(&ctx, true, 1);
    let expired = stored_plan(&ctx, false, 31);
    let st = ctx.store().load().unwrap();
    assert!(check_plan_usable(&st.plans[&fresh], Utc::now()).is_ok());

    let apply = |id: &str| deploy::apply(&ctx, &json!({ "plan_id": id })).unwrap_err().to_string();
    assert!(apply(&applied).contains("already applied"));
    assert!(apply(&expired).contains("expired"));
    assert!(apply("not-a-plan").contains("only accepts a plan_id"));
}

#[test]
fn apply_without_docker_is_an_error_and_releases_the_lock() {
    let ctx = no_docker_ctx();
    let id = stored_plan(&ctx, false, 0);
    let err = deploy::apply(&ctx, &json!({ "plan_id": id })).unwrap_err();
    assert!(format!("{err:#}").contains("could not run"), "{err:#}");
    let st = ctx.store().load().unwrap();
    assert!(!st.plans[&id].applied);
    assert!(st.deployments.is_empty());
    assert!(!crate::is_locked(&st, "planned-app", Utc::now()));
}

#[test]
fn busy_lock_blocks_a_second_holder_until_dropped() {
    let ctx = no_docker_ctx();
    let lock = BusyLock::acquire(&ctx, "app1").unwrap();
    assert!(crate::is_locked(&ctx.store().load().unwrap(), "app1", Utc::now()));
    assert!(BusyLock::acquire(&ctx, "app1").unwrap_err().to_string().contains("busy"));
    assert!(BusyLock::acquire(&ctx, "app2").is_ok());
    drop(lock);
    assert!(!crate::is_locked(&ctx.store().load().unwrap(), "app1", Utc::now()));
    assert!(BusyLock::acquire(&ctx, "app1").is_ok());
}

// ------------------------------------------------------------ destroy / targets

#[test]
fn destroy_requires_confirm_equal_to_app() {
    let ctx = no_docker_ctx();
    let mut st = State::default();
    st.deployments.insert("web".into(), deployment("web", "local", 100, 64, None, Some(1)));
    ctx.store().save(&st).unwrap();
    for args in [json!({ "app": "web" }), json!({ "app": "web", "confirm": "Web" }), json!({ "app": "web", "confirm": "other" })] {
        let err = deploy::destroy(&ctx, &args).unwrap_err().to_string();
        assert!(err.contains("confirm"), "{err}");
    }
    assert!(ctx.store().load().unwrap().deployments.contains_key("web"));
    // Right confirm but no Docker: error, record kept.
    assert!(deploy::destroy(&ctx, &json!({ "app": "web", "confirm": "web" })).is_err());
    assert!(ctx.store().load().unwrap().deployments.contains_key("web"));
}

fn remote(id: &str, provider: &str) -> Target {
    Target {
        id: id.into(),
        docker_host: Some(format!("ssh://u@{id}")),
        address: id.into(),
        provider: provider.into(),
        ..Target::local()
    }
}

#[test]
fn target_remove_guards() {
    let mut ctx = no_docker_ctx();
    let log = fake(&mut ctx, vec![]);
    let mut st = State::default();
    st.targets.push(remote("busy", "ssh"));
    st.targets.push(remote("plain", "ssh"));
    st.targets.push(remote("cloudy", "fake"));
    st.deployments.insert("web".into(), deployment("web", "busy", 100, 64, None, Some(1)));
    ctx.store().save(&st).unwrap();

    let rm = |args: serde_json::Value| targets::remove(&ctx, &args);
    assert!(rm(json!({ "id": "local" })).unwrap_err().to_string().contains("cannot be removed"));
    assert!(rm(json!({ "id": "busy" })).unwrap_err().to_string().contains("web"));
    assert!(rm(json!({ "id": "nope" })).is_err());
    assert!(rm(json!({ "id": "cloudy" })).unwrap_err().to_string().contains("confirm"));
    assert!(rm(json!({ "id": "cloudy", "confirm": "plain" })).is_err());
    assert!(log.lock().unwrap().is_empty());

    let out = rm(json!({ "id": "cloudy", "confirm": "cloudy" })).unwrap();
    assert_eq!(out["removed"], true);
    assert_eq!(*log.lock().unwrap(), vec!["cloudy".to_string()]);
    assert_eq!(rm(json!({ "id": "plain" })).unwrap()["deprovisioned"], false);

    let ids: Vec<String> = ctx.store().load().unwrap().targets.into_iter().map(|t| t.id).collect();
    assert_eq!(ids, vec!["busy".to_string()]);
}

#[test]
fn target_add_saves_nothing_when_docker_cannot_reach_it() {
    let ctx = no_docker_ctx();
    let err = targets::add(&ctx, &json!({ "id": "edge-1", "docker_host": "ssh://u@h", "address": "h" })).unwrap_err().to_string();
    assert!(err.contains("not added"), "{err}");
    assert!(ctx.store().load().unwrap().targets.is_empty());
    assert!(targets::add(&ctx, &json!({ "id": "local", "docker_host": "ssh://u@h", "address": "h" })).is_err());
    assert!(targets::add(&ctx, &json!({ "id": "Bad Id", "docker_host": "ssh://u@h", "address": "h" })).is_err());
}

#[test]
fn targets_list_reports_unreachable_without_capacity() {
    let ctx = no_docker_ctx();
    let out = targets::list(&ctx, &json!({})).unwrap();
    let t = &out["targets"][0];
    assert_eq!(t["id"], "local");
    assert_eq!(t["reachable"], false);
    assert!(t.get("capacity").is_none());
    assert!(t["error"].as_str().unwrap().contains("could not run"));
}

// ------------------------------------------------------------ rollback / run args

#[test]
fn rollback_picks_latest_superseded_before_current() {
    let mut d = deployment("web", "local", 100, 64, None, None);
    for (n, s) in [(1, "superseded"), (2, "superseded"), (3, "failed"), (4, "active")] {
        d.releases.push(Release { number: n, image: format!("ltn/web:r{n}"), plan_id: None, created_at: Utc::now(), state: s.into() });
    }
    d.current = Some(4);
    assert_eq!(rollback_target(&d, None).unwrap(), 2);
    assert_eq!(rollback_target(&d, Some(1)).unwrap(), 1);
    assert!(rollback_target(&d, Some(3)).is_err()); // failed release
    assert!(rollback_target(&d, Some(4)).is_err()); // already current
    assert!(rollback_target(&d, Some(9)).is_err());
    let lone = deployment("x", "local", 1, 16, None, Some(1));
    assert!(rollback_target(&lone, None).is_err());
}

#[test]
fn run_args_follow_conventions() {
    let mut s = spec("web", 500, 256, Some(18195));
    s.env.insert("MODE".into(), "prod".into());
    let a = run_args(&s, "web", 3, "ltn/web:r3").join(" ");
    for part in [
        "run -d --name ltn-web-r3",
        "--label ltn.app=web",
        "--label ltn.release=3",
        "--restart unless-stopped",
        "--cpus 0.500",
        "--memory 256m",
        "-p 18195:8080",
        "-e MODE=prod",
    ] {
        assert!(a.contains(part), "missing `{part}` in `{a}`");
    }
    assert!(a.ends_with("ltn/web:r3"));
}

#[test]
fn tools_cover_the_engine_contract() {
    let names: Vec<&str> = crate::tools().iter().map(|t| t.name).collect();
    for n in ["ltn_analyze", "ltn_plan", "ltn_apply", "ltn_rollback", "ltn_destroy", "ltn_targets", "ltn_target_add", "ltn_target_remove"] {
        assert!(names.contains(&n), "{n}");
    }
    let dir = project("host_port = 18196\n");
    let out = plan::analyze_tool(&no_docker_ctx(), &json!({ "path": dir })).unwrap();
    assert_eq!(out["host_port"], 18196);
}
