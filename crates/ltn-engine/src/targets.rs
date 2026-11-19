//! ltn_targets, ltn_target_add, ltn_target_remove.

use crate::capacity::{capacity_json, probe_target};
use anyhow::{anyhow, bail, Result};
use ltn_state::spec::sanitize_name;
use ltn_state::store::Target;
use ltn_state::tool::{arg_str, arg_str_opt, arg_u64_opt};
use ltn_state::Ctx;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn list(ctx: &Ctx, _args: &Value) -> Result<Value> {
    let state = ctx.store().load()?;
    let mut out = Vec::new();
    for t in state.all_targets() {
        let mut row = json!({
            "id": t.id,
            "provider": t.provider,
            "docker_host": t.docker_host,
            "address": t.address,
            "cost_cents_per_hour": t.cost_cents_per_hour,
            "deployments": state.deployments.values().filter(|d| d.target_id == t.id).map(|d| d.app.clone()).collect::<Vec<_>>(),
        });
        match probe_target(ctx, &t) {
            Ok((info, total)) => {
                row["reachable"] = json!(true);
                row["capacity"] = capacity_json(&state, &t, &info, &total);
            }
            Err(e) => {
                row["reachable"] = json!(false);
                row["error"] = json!(e);
            }
        }
        out.push(row);
    }
    Ok(json!({ "targets": out }))
}

pub fn add(ctx: &Ctx, args: &Value) -> Result<Value> {
    let raw_id = arg_str(args, "id")?;
    let id = sanitize_name(raw_id);
    if id != raw_id {
        bail!("target id `{raw_id}` must be lowercase letters, digits and dashes (e.g. `{id}`)");
    }
    if id == "local" {
        bail!("`local` is reserved for the local Docker daemon");
    }
    let docker_host = arg_str(args, "docker_host")?.trim().to_string();
    let address = arg_str(args, "address")?.trim().to_string();
    let target = Target {
        id: id.clone(),
        docker_host: Some(docker_host.clone()),
        address,
        cost_cents_per_hour: arg_u64_opt(args, "cost_cents_per_hour").unwrap_or(0),
        gpu_count: arg_u64_opt(args, "gpu_count").unwrap_or(0),
        vram_mb: arg_u64_opt(args, "vram_mb").unwrap_or(0),
        provider: "ssh".into(),
        labels: BTreeMap::new(),
    };
    let store = ctx.store();
    if store.load()?.target(&id).is_some() {
        bail!("target `{id}` already exists; remove it first with ltn_target_remove");
    }
    // Verify the daemon answers before anything is saved.
    let (info, total) = probe_target(ctx, &target).map_err(|e| {
        anyhow!(
            "target `{id}` not added: `docker -H {docker_host} info` failed: {e}. Check that the host is \
             reachable over SSH with your agent/key (try `ssh` to it) and that the remote user can run docker."
        )
    })?;
    store.update(|st| {
        if st.target(&target.id).is_some() {
            bail!("target `{}` already exists", target.id);
        }
        st.targets.push(target.clone());
        Ok(())
    })?;
    let state = store.load()?;
    store.audit(None, "target_add", json!({ "id": id, "docker_host": docker_host, "cpus": info.cpus, "memory_mb": info.memory_mb }));
    Ok(json!({
        "id": id,
        "provider": "ssh",
        "reachable": true,
        "capacity": capacity_json(&state, &target, &info, &total),
    }))
}

pub fn remove(ctx: &Ctx, args: &Value) -> Result<Value> {
    let id = arg_str(args, "id")?;
    if id == "local" {
        bail!("the local target cannot be removed");
    }
    let store = ctx.store();
    let state = store.load()?;
    let target = state
        .targets
        .iter()
        .find(|t| t.id == id)
        .cloned()
        .ok_or_else(|| anyhow!("no target `{id}`. See ltn_targets."))?;
    let apps: Vec<String> = state.deployments.values().filter(|d| d.target_id == id).map(|d| d.app.clone()).collect();
    if !apps.is_empty() {
        bail!("target `{id}` still hosts deployments: {}. Destroy them (ltn_destroy) first.", apps.join(", "));
    }
    let provisioner = ctx.provisioners.iter().find(|p| p.provider() == target.provider).cloned();
    let mut deprovisioned = false;
    if let Some(p) = provisioner {
        if arg_str_opt(args, "confirm") != Some(id) {
            bail!(
                "target `{id}` is {} capacity; removing it destroys the machine. Pass confirm = \"{id}\" to proceed.",
                target.provider
            );
        }
        p.deprovision(ctx, &target)
            .map_err(|e| anyhow!("deprovisioning `{id}` failed, target kept: {e}"))?;
        deprovisioned = true;
    }
    store.update(|st| {
        st.targets.retain(|t| t.id != id);
        Ok(())
    })?;
    store.audit(None, "target_remove", json!({ "id": id, "provider": target.provider, "deprovisioned": deprovisioned }));
    Ok(json!({ "id": id, "removed": true, "deprovisioned": deprovisioned }))
}
