//! Cloud provisioner: instance catalog and OpenTofu-provisioned Docker hosts.
//!
//! A cloud target is a VM with Docker and SSH, created by OpenTofu and then
//! driven like any other Docker host via `docker -H ssh://ltn-<name>`.
//! Credentials come only from the AWS CLI's standard credential chain.

pub mod auth;
pub mod catalog;
pub mod exec;
pub mod hcl;
pub mod provision;
pub mod sshcfg;

pub use provision::{AwsProvisioner, ProvisionEnv};

use anyhow::{anyhow, Result};
use ltn_state::tool::{arg_bool, arg_str_opt};
use ltn_state::{Ctx, Provisioner, ToolDef};
use serde_json::{json, Value};
use std::sync::Arc;

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ltn_cloud_auth",
            description: "Check whether AWS credentials are available to the ltn server through the standard AWS \
                          credential chain (runs `aws sts get-caller-identity`). Never takes keys as arguments.",
            input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            handler: cloud_auth,
        },
        ToolDef {
            name: "ltn_cloud_catalog",
            description: "AWS instance catalog (vCPU, memory, GPUs, on-demand Linux price) used for cloud placement. \
                          refresh=true fetches it from AWS (needs credentials) and caches it; otherwise returns the \
                          cached catalog. There are no built-in prices.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "refresh": { "type": "boolean", "description": "Fetch from AWS and update the cache." },
                    "region": { "type": "string", "description": "AWS region, e.g. us-east-1. Defaults to AWS_REGION / the CLI's configured region." },
                    "families": { "type": "array", "items": { "type": "string" }, "description": "Instance family allowlist for refresh (default t3,t3a,m6i,m7i,c6i,c7i,g4dn,g5 or LTN_AWS_FAMILIES)." }
                },
                "additionalProperties": false
            }),
            handler: cloud_catalog,
        },
        ToolDef {
            name: "ltn_cloud_targets",
            description: "List cloud infrastructure ltn has created (name, instance type, region, public IP, module dir, created_at, hourly cost, status).",
            input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            handler: cloud_targets,
        },
    ]
}

/// Provisioners to register in `Ctx::provisioners`.
pub fn provisioners() -> Vec<Arc<dyn Provisioner>> {
    vec![Arc::new(AwsProvisioner)]
}

fn cloud_auth(_ctx: &Ctx, _args: &Value) -> Result<Value> {
    let bin = exec::aws_bin();
    Ok(match auth::check(&bin) {
        auth::AuthStatus::Configured(id) => json!({
            "configured": true,
            "account": id.account,
            "arn": id.arn,
            "region": auth::default_region(&bin),
        }),
        auth::AuthStatus::Unconfigured(reason) => json!({ "configured": false, "reason": reason }),
    })
}

fn catalog_json(cat: &catalog::Catalog) -> Value {
    let offers: Vec<Value> = cat
        .entries
        .iter()
        .map(|e| {
            json!({
                "offer_id": catalog::offer_id(&e.spec.instance_type, &cat.region),
                "instance_type": e.spec.instance_type,
                "vcpus": e.spec.vcpus,
                "memory_mib": e.spec.memory_mib,
                "gpu_count": e.spec.gpu_count,
                "gpu_vram_mib": e.spec.gpu_vram_mib,
                "gpu_name": e.spec.gpu_name,
                "usd_per_hour": e.usd_per_hour,
                "cost_cents_per_hour": e.cents_per_hour,
            })
        })
        .collect();
    json!({
        "region": cat.region,
        "fetched_at": cat.fetched_at.to_rfc3339(),
        "families": cat.families,
        "count": offers.len(),
        "entries": offers,
        "unpriced": cat.unpriced,
    })
}

fn cloud_catalog(ctx: &Ctx, args: &Value) -> Result<Value> {
    let bin = exec::aws_bin();
    let region_arg = arg_str_opt(args, "region").map(str::to_string);
    if arg_bool(args, "refresh") {
        if let auth::AuthStatus::Unconfigured(reason) = auth::check(&bin) {
            return Err(anyhow!("cannot refresh the AWS catalog: credentials are not configured. {reason}"));
        }
        let region = region_arg.or_else(|| auth::default_region(&bin)).ok_or_else(|| {
            anyhow!("no AWS region given and none configured; pass region (e.g. \"us-east-1\") or set AWS_REGION")
        })?;
        let families: Vec<String> = match args.get("families").and_then(Value::as_array) {
            Some(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
            None => catalog::families_from_env(),
        };
        // Network calls happen on no lock; only the result is recorded under update.
        let cat = catalog::fetch(&bin, &region, &families)?;
        let store = ctx.store();
        store.update(|st| catalog::store_catalog(st, &cat))?;
        store.audit(None, "cloud_catalog_refresh", json!({ "region": region, "entries": cat.entries.len() }));
        let mut out = catalog_json(&cat);
        out["source"] = json!("aws");
        return Ok(out);
    }
    let state = ctx.store().load()?;
    let mut cats = catalog::cached_catalogs(&state);
    if let Some(r) = &region_arg {
        cats.retain(|c| &c.region == r);
    }
    if cats.is_empty() {
        return Ok(json!({
            "source": "cache",
            "count": 0,
            "catalogs": [],
            "note": "No cached AWS catalog. ltn has no built-in prices: call ltn_cloud_catalog with refresh=true \
                     (and region) once AWS credentials are configured (check with ltn_cloud_auth).",
        }));
    }
    let count: usize = cats.iter().map(|c| c.entries.len()).sum();
    Ok(json!({
        "source": "cache",
        "count": count,
        "catalogs": cats.iter().map(catalog_json).collect::<Vec<_>>(),
    }))
}

fn cloud_targets(ctx: &Ctx, _args: &Value) -> Result<Value> {
    let state = ctx.store().load()?;
    let targets: Vec<Value> = provision::infra_records(&state)
        .into_iter()
        .map(|r| {
            json!({
                "name": r.name,
                "status": r.status,
                "offer_id": r.offer_id,
                "instance_type": r.instance_type,
                "region": r.region,
                "instance_id": r.instance_id,
                "public_ip": r.public_ip,
                "docker_host": r.docker_host,
                "module_dir": r.module_dir,
                "created_at": r.created_at.to_rfc3339(),
                "cost_cents_per_hour": r.cost_cents_per_hour,
                "usd_per_hour": r.usd_per_hour,
            })
        })
        .collect();
    Ok(json!({ "count": targets.len(), "targets": targets }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler(name: &str) -> ltn_state::Handler {
        tools().into_iter().find(|t| t.name == name).unwrap().handler
    }

    #[test]
    fn tool_names_and_provisioner() {
        let names: Vec<&str> = tools().iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["ltn_cloud_auth", "ltn_cloud_catalog", "ltn_cloud_targets"]);
        let p = provisioners();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].provider(), "aws");
    }

    #[test]
    fn empty_catalog_and_targets_say_how_to_populate() {
        let home = std::env::temp_dir().join(format!("ltn-cloud-lib-{}", uuid::Uuid::new_v4().simple()));
        let ctx = Ctx::for_home(&home);
        let v = handler("ltn_cloud_catalog")(&ctx, &json!({})).unwrap();
        assert_eq!(v["count"], 0);
        assert!(v["note"].as_str().unwrap().contains("refresh=true"));
        let v = handler("ltn_cloud_targets")(&ctx, &json!({})).unwrap();
        assert_eq!(v["count"], 0);
        let _ = std::fs::remove_dir_all(&home);
    }
}
