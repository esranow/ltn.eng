//! ltn_analyze and ltn_plan: read the project, gather candidate capacity
//! (existing targets plus provisioner offers) and let Z3 choose.

use crate::capacity::{budget_cap, env_budget, free_capacity, probe_target, reserved_on};
use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, Utc};
use ltn_state::spec::{analyze, AppSpec};
use ltn_state::store::{Plan, State};
use ltn_state::tool::{arg_str, arg_str_opt};
use ltn_state::{place, Ctx, Offer, PlacementDecision, PlacementError, PlacementRequest, TargetCapacity};
use serde_json::{json, Value};
use std::path::Path;

/// How long a plan stays applicable.
pub const PLAN_TTL_MINUTES: i64 = 30;

pub fn analyze_tool(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let spec = analyze(Path::new(arg_str(args, "path")?))?;
    Ok(serde_json::to_value(spec)?)
}

pub fn plan_tool(ctx: &Ctx, args: &Value) -> Result<Value> {
    plan_with_env_cap(ctx, args, env_budget().as_deref())
}

pub fn request_for(spec: &AppSpec, budget: u64) -> PlacementRequest {
    PlacementRequest {
        cpu_millis: spec.cpu_millis,
        ram_mb: spec.ram_mb,
        gpu_count: spec.gpu_count,
        vram_mb: spec.vram_mb,
        max_cost_cents_per_hour: budget,
    }
}

fn cap_line(c: &TargetCapacity) -> String {
    format!(
        "{}: free cpu_millis={} ram_mb={} gpu={} vram_mb={} cost_cents_per_hour={}",
        c.target_id, c.free_cpu_millis, c.free_ram_mb, c.free_gpu_count, c.free_vram_mb, c.cost_cents_per_hour
    )
}

/// Runs the Z3 placement over existing capacity and offers. Returns the
/// decision, the offer chosen (if the winner does not exist yet) and the full
/// candidate list. Unsatisfiable explains the unsat core next to each
/// candidate's free capacity.
pub fn solve(
    spec: &AppSpec,
    existing: &[TargetCapacity],
    offers: &[Offer],
    budget: u64,
) -> Result<(PlacementDecision, Option<Offer>, Vec<TargetCapacity>)> {
    let mut candidates: Vec<TargetCapacity> = existing.to_vec();
    for o in offers {
        let mut c = o.capacity.clone();
        c.target_id = o.offer_id.clone();
        candidates.push(c);
    }
    let req = request_for(spec, budget);
    match place(&req, &candidates) {
        Ok(d) => {
            let offer = if existing.iter().any(|c| c.target_id == d.target_id) {
                None
            } else {
                offers.iter().find(|o| o.offer_id == d.target_id).cloned()
            };
            Ok((d, offer, candidates))
        }
        Err(PlacementError::Unsatisfiable { unsat_core }) => {
            let budget_txt = if budget == u64::MAX { "unlimited".to_string() } else { budget.to_string() };
            bail!(
                "no candidate can fit app `{}`. Z3 unsat core (conflicting constraints): [{}].\n\
                 Requested: cpu_millis={} ram_mb={} gpu={} vram_mb={} max_cost_cents_per_hour={}.\n\
                 Candidates considered:\n  {}\n\
                 Next: lower cpu/ram_mb/gpu in ltn.toml, free capacity on a target, add a target \
                 (ltn_target_add), or raise max_cost_cents_per_hour / LTN_MAX_HOURLY_CENTS if `budget` is in the core.",
                spec.name,
                unsat_core.join(", "),
                req.cpu_millis,
                req.ram_mb,
                req.gpu_count,
                req.vram_mb,
                budget_txt,
                candidates.iter().map(cap_line).collect::<Vec<_>>().join("\n  ")
            )
        }
        Err(PlacementError::NoTargets) => bail!("no candidate capacity to place `{}` on", spec.name),
        Err(e) => bail!("placement of `{}` failed: {e}", spec.name),
    }
}

/// Another app already publishing `host_port` on `target_id`, if any.
pub fn host_port_conflict(state: &State, target_id: &str, app: &str, host_port: Option<u16>) -> Option<String> {
    let hp = host_port?;
    state
        .deployments
        .values()
        .find(|d| d.target_id == target_id && d.app != app && d.current.is_some() && d.spec.host_port == Some(hp))
        .map(|d| d.app.clone())
}

pub fn expires_at(plan: &Plan) -> DateTime<Utc> {
    plan.created_at + Duration::minutes(PLAN_TTL_MINUTES)
}

/// A plan can be applied once, within its TTL.
pub fn check_plan_usable(plan: &Plan, now: DateTime<Utc>) -> Result<()> {
    if plan.applied {
        bail!("plan `{}` was already applied. Run ltn_plan again for a new release.", plan.id);
    }
    if now >= expires_at(plan) {
        bail!(
            "plan `{}` expired at {} (plans are valid for {PLAN_TTL_MINUTES} minutes). Run ltn_plan again.",
            plan.id,
            expires_at(plan).to_rfc3339()
        );
    }
    Ok(())
}

/// ltn_plan with the env budget cap passed explicitly (tests avoid process env).
pub fn plan_with_env_cap(ctx: &Ctx, args: &Value, env_cap: Option<&str>) -> Result<Value> {
    let spec = analyze(Path::new(arg_str(args, "path")?))?;
    let pin = arg_str_opt(args, "target");
    let budget = budget_cap(spec.max_cost_cents_per_hour, env_cap)?;
    let store = ctx.store();
    let state = store.load()?;
    let app = spec.name.clone();

    let mut existing = Vec::new();
    let mut unreachable = Vec::new();
    let mut excluded = Vec::new();
    for t in state.all_targets() {
        if pin.map_or(false, |p| p != t.id) {
            continue;
        }
        if let Some(other) = host_port_conflict(&state, &t.id, &app, spec.host_port) {
            excluded.push(json!({
                "id": t.id,
                "reason": format!("host_port {} is already published by app `{other}` on this target",
                    spec.host_port.unwrap_or(0)),
            }));
            continue;
        }
        match probe_target(ctx, &t) {
            Err(e) => unreachable.push(json!({ "id": t.id, "error": e })),
            Ok((_info, total)) => existing.push(free_capacity(&total, reserved_on(&state, &t.id, Some(&app)))),
        }
    }

    let mut offers = Vec::new();
    let mut offer_errors = Vec::new();
    for p in &ctx.provisioners {
        match p.offers(ctx) {
            Ok(list) => offers.extend(list.into_iter().filter(|o| pin.map_or(true, |p| p == o.offer_id))),
            Err(e) => offer_errors.push(json!({ "provider": p.provider(), "error": e.to_string() })),
        }
    }

    let context = json!({ "unreachable_targets": unreachable, "excluded_targets": excluded, "offer_errors": offer_errors });
    if existing.is_empty() && offers.is_empty() {
        if let Some(p) = pin {
            if state.target(p).is_none() {
                bail!("unknown target `{p}`: not a registered target or a provisioner offer. See ltn_targets.");
            }
        }
        bail!(
            "no usable capacity to place `{app}`{}. Details: {context}. Next: start Docker / fix the \
             unreachable target, pick a different host_port, or add a target with ltn_target_add.",
            pin.map(|p| format!(" on target `{p}`")).unwrap_or_default()
        );
    }

    let (decision, offer, considered) = match solve(&spec, &existing, &offers, budget) {
        Ok(v) => v,
        Err(e) => {
            if unreachable.is_empty() && excluded.is_empty() && offer_errors.is_empty() {
                return Err(e);
            }
            return Err(anyhow!("{e}\nAlso not considered: {context}"));
        }
    };

    let now = Utc::now();
    let plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now,
        spec: spec.clone(),
        target_id: decision.target_id.clone(),
        provision_offer: offer.as_ref().map(|o| o.offer_id.clone()),
        decision: decision.clone(),
        capacity_considered: considered.clone(),
        applied: false,
    };
    store.update(|st| {
        // Drop plans nobody can apply any more so state does not grow forever.
        st.plans.retain(|_, p| now < expires_at(p) + Duration::hours(24));
        st.plans.insert(plan.id.clone(), plan.clone());
        Ok(())
    })?;
    store.audit(
        Some(&app),
        "plan",
        json!({ "plan_id": plan.id, "target_id": plan.target_id, "provision_offer": plan.provision_offer,
                "cost_cents_per_hour": decision.cost_cents_per_hour }),
    );

    Ok(json!({
        "plan_id": plan.id,
        "app": app,
        "target_id": plan.target_id,
        "provision_offer": plan.provision_offer,
        "offer": offer,
        "cost_cents_per_hour": decision.cost_cents_per_hour,
        "ram_mb_left_free": decision.ram_mb_left_free,
        "budget_cents_per_hour": if budget == u64::MAX { Value::Null } else { json!(budget) },
        "request": request_for(&spec, budget),
        "capacity_considered": considered,
        "unreachable_targets": context["unreachable_targets"],
        "excluded_targets": context["excluded_targets"],
        "offer_errors": context["offer_errors"],
        "expires_at": expires_at(&plan).to_rfc3339(),
        "spec": spec,
        "smt2": decision.smt2,
    }))
}
