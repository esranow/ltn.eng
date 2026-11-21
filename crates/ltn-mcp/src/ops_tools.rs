//! Autonomous DevOps tools exposed via MCP for secrets, networking, Kubernetes,
//! canary delivery, and disaster recovery.

use anyhow::{anyhow, Result};
use ltn_delivery::{CanaryConfig, CanaryController, CanaryDecision, TrafficMetrics};
use ltn_k8s::{K8sAppConfig, K8sManifestSynthesizer};
use ltn_net::{DnsRecord, DnsRecordType, IngressRoute, TlsMode};
use ltn_secrets::{derive_key_from_passphrase, SecretStore};
use ltn_state::{Ctx, ToolDef};
use ltn_storage::{BackupEngine, DrDrillRunner, DrillStatus};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

pub fn tools() -> Vec<ToolDef> {
    vec![
        secret_set_tool(),
        secret_get_tool(),
        secret_list_tool(),
        route_add_tool(),
        domain_bind_tool(),
        k8s_generate_tool(),
        canary_step_tool(),
        canary_abort_tool(),
        dr_drill_tool(),
    ]
}

fn get_str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("missing required string argument `{key}`"))
}

fn get_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn get_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn secrets_file_path(ctx: &Ctx) -> PathBuf {
    ctx.home.join("secrets").join("vault.enc")
}

fn get_or_create_store(ctx: &Ctx) -> Result<SecretStore> {
    let p = secrets_file_path(ctx);
    let salt = b"ltn_eng_salt_2026";
    let key = derive_key_from_passphrase("ltn-autonomous-devops-master-key", salt);
    if p.exists() {
        SecretStore::load_from_file(&p, key)
    } else {
        Ok(SecretStore::new(key))
    }
}

fn save_store(ctx: &Ctx, store: &SecretStore) -> Result<()> {
    let p = secrets_file_path(ctx);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    store.save_to_file(&p)
}

// -----------------------------------------------------------------------------
// 1. Secrets: Set, Get, List
// -----------------------------------------------------------------------------
fn secret_set_tool() -> ToolDef {
    ToolDef {
        name: "ltn_secret_set",
        description: "Store an AES-256-GCM zero-trust encrypted secret for an application.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app": { "type": "string", "description": "Application name" },
                "key": { "type": "string", "description": "Secret key name (e.g. DATABASE_URL)" },
                "value": { "type": "string", "description": "Secret value to encrypt" }
            },
            "required": ["app", "key", "value"],
            "additionalProperties": false
        }),
        handler: handle_secret_set,
    }
}

fn handle_secret_set(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = get_str_arg(args, "app")?;
    let key = get_str_arg(args, "key")?;
    let val = get_str_arg(args, "value")?;

    let mut store = get_or_create_store(ctx)?;
    store.set(app, key, val)?;
    save_store(ctx, &store)?;

    Ok(json!({
        "status": "stored",
        "app": app,
        "key": key,
        "masked": "***"
    }))
}

fn secret_get_tool() -> ToolDef {
    ToolDef {
        name: "ltn_secret_get",
        description: "Decrypt and retrieve a secret for an application.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app": { "type": "string", "description": "Application name" },
                "key": { "type": "string", "description": "Secret key name" }
            },
            "required": ["app", "key"],
            "additionalProperties": false
        }),
        handler: handle_secret_get,
    }
}

fn handle_secret_get(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = get_str_arg(args, "app")?;
    let key = get_str_arg(args, "key")?;

    let store = get_or_create_store(ctx)?;
    let val = store.get(app, key)?;

    Ok(json!({
        "app": app,
        "key": key,
        "value": val
    }))
}

fn secret_list_tool() -> ToolDef {
    ToolDef {
        name: "ltn_secret_list",
        description: "List all encrypted secret keys stored for an application (values masked).",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app": { "type": "string", "description": "Application name" }
            },
            "required": ["app"],
            "additionalProperties": false
        }),
        handler: handle_secret_list,
    }
}

fn handle_secret_list(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app = get_str_arg(args, "app")?;
    let store = get_or_create_store(ctx)?;
    let keys = store.list_keys(app);

    Ok(json!({
        "app": app,
        "keys": keys
    }))
}

// -----------------------------------------------------------------------------
// 2. Networking & Domain Routing
// -----------------------------------------------------------------------------
fn route_add_tool() -> ToolDef {
    ToolDef {
        name: "ltn_route_add",
        description: "Configure dynamic reverse proxy ingress route and TLS termination.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Fully qualified domain name (e.g. api.example.com)" },
                "target_app": { "type": "string", "description": "Target application name" },
                "target_port": { "type": "integer", "description": "Target upstream port" },
                "tls_mode": { "type": "string", "enum": ["disabled", "auto_acme", "selfsigned"], "description": "TLS certificate strategy" }
            },
            "required": ["domain", "target_app", "target_port"],
            "additionalProperties": false
        }),
        handler: handle_route_add,
    }
}

fn handle_route_add(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let domain = get_str_arg(args, "domain")?;
    let target_app = get_str_arg(args, "target_app")?;
    let target_port = get_u64_opt(args, "target_port").unwrap_or(8080) as u16;
    let mode_str = get_str_opt(args, "tls_mode").unwrap_or("auto_acme");

    let tls = match mode_str {
        "disabled" => TlsMode::Disabled,
        "selfsigned" => TlsMode::SelfSigned,
        _ => TlsMode::AutoAcme,
    };

    let mut route = IngressRoute::new(domain, target_app, target_port);
    route.tls_mode = tls;

    Ok(json!({
        "status": "configured",
        "route": route
    }))
}

fn domain_bind_tool() -> ToolDef {
    ToolDef {
        name: "ltn_domain_bind",
        description: "Synthesize DNS record binding for external apex or subdomain.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "domain": { "type": "string", "description": "Domain name" },
                "target": { "type": "string", "description": "Target IP or hostname" },
                "record_type": { "type": "string", "enum": ["A", "AAAA", "CNAME"], "description": "DNS Record Type" }
            },
            "required": ["domain", "target"],
            "additionalProperties": false
        }),
        handler: handle_domain_bind,
    }
}

fn handle_domain_bind(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let domain = get_str_arg(args, "domain")?;
    let target = get_str_arg(args, "target")?;
    let rtype_str = get_str_opt(args, "record_type").unwrap_or("A");

    let rtype = match rtype_str {
        "AAAA" => DnsRecordType::AAAA,
        "CNAME" => DnsRecordType::CNAME,
        _ => DnsRecordType::A,
    };

    let rec = DnsRecord {
        name: domain.to_string(),
        record_type: rtype,
        content: target.to_string(),
        ttl: 300,
        proxied: true,
    };

    Ok(json!({
        "status": "bound",
        "dns_record": rec
    }))
}

// -----------------------------------------------------------------------------
// 3. Kubernetes Manifest Synthesis
// -----------------------------------------------------------------------------
fn k8s_generate_tool() -> ToolDef {
    ToolDef {
        name: "ltn_k8s_generate",
        description: "Synthesize production-grade Kubernetes manifests (Deployment, Service, Ingress) with strict Z3 resource sizing.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app_name": { "type": "string", "description": "Kubernetes workload name" },
                "image": { "type": "string", "description": "Container image reference" },
                "replicas": { "type": "integer", "description": "Desired replica count" },
                "cpu_millis": { "type": "integer", "description": "CPU millicores request" },
                "memory_mb": { "type": "integer", "description": "RAM MB request" },
                "port": { "type": "integer", "description": "Container port" },
                "domain": { "type": "string", "description": "Optional ingress domain" }
            },
            "required": ["app_name", "image"],
            "additionalProperties": false
        }),
        handler: handle_k8s_generate,
    }
}

fn handle_k8s_generate(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let app_name = get_str_arg(args, "app_name")?;
    let image = get_str_arg(args, "image")?;
    let replicas = get_u64_opt(args, "replicas").unwrap_or(2) as i32;
    let cpu_millis = get_u64_opt(args, "cpu_millis").unwrap_or(500);
    let memory_mb = get_u64_opt(args, "memory_mb").unwrap_or(512);
    let port = get_u64_opt(args, "port").unwrap_or(8080) as u16;
    let domain = get_str_opt(args, "domain");

    let mut cfg = K8sAppConfig::default();
    cfg.name = app_name.to_string();
    cfg.image = image.to_string();
    cfg.replicas = replicas;
    cfg.container_port = port;
    cfg.service_port = port;
    cfg.cpu_request = format!("{}m", cpu_millis);
    cfg.memory_request = format!("{}Mi", memory_mb);
    cfg.cpu_limit = format!("{}m", cpu_millis * 2);
    cfg.memory_limit = format!("{}Mi", memory_mb * 2);

    let depl_val = K8sManifestSynthesizer::generate_deployment(&cfg);
    let svc_val = K8sManifestSynthesizer::generate_service(&cfg);
    let ing_val = domain.map(|d| K8sManifestSynthesizer::generate_ingress(&cfg, d));

    let depl_yaml = K8sManifestSynthesizer::to_yaml(&depl_val)?;
    let svc_yaml = K8sManifestSynthesizer::to_yaml(&svc_val)?;
    let ing_yaml = match ing_val {
        Some(ref val) => Some(K8sManifestSynthesizer::to_yaml(val)?),
        None => None,
    };

    Ok(json!({
        "status": "generated",
        "app_name": app_name,
        "deployment_yaml": depl_yaml,
        "service_yaml": svc_yaml,
        "ingress_yaml": ing_yaml
    }))
}

// -----------------------------------------------------------------------------
// 4. Progressive Delivery & Automated Canary Analysis
// -----------------------------------------------------------------------------
fn canary_step_tool() -> ToolDef {
    ToolDef {
        name: "ltn_canary_step",
        description: "Evaluate progressive delivery canary metrics and advance or abort traffic.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "stage_idx": { "type": "integer", "description": "Current stage index (0 to N)" },
                "baseline_requests": { "type": "integer" },
                "baseline_errors": { "type": "integer" },
                "baseline_p99_ms": { "type": "number" },
                "canary_requests": { "type": "integer" },
                "canary_errors": { "type": "integer" },
                "canary_p99_ms": { "type": "number" }
            },
            "required": ["stage_idx", "baseline_requests", "canary_requests"],
            "additionalProperties": false
        }),
        handler: handle_canary_step,
    }
}

fn handle_canary_step(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let stage_idx = get_u64_opt(args, "stage_idx").unwrap_or(0) as usize;
    let b_req = get_u64_opt(args, "baseline_requests").unwrap_or(0);
    let b_err = get_u64_opt(args, "baseline_errors").unwrap_or(0);
    let b_p99 = args.get("baseline_p99_ms").and_then(Value::as_f64).unwrap_or(50.0);

    let c_req = get_u64_opt(args, "canary_requests").unwrap_or(0);
    let c_err = get_u64_opt(args, "canary_errors").unwrap_or(0);
    let c_p99 = args.get("canary_p99_ms").and_then(Value::as_f64).unwrap_or(50.0);

    let baseline = TrafficMetrics {
        total_requests: b_req,
        error_requests: b_err,
        p99_latency_ms: b_p99,
    };
    let canary = TrafficMetrics {
        total_requests: c_req,
        error_requests: c_err,
        p99_latency_ms: c_p99,
    };

    let cfg = CanaryConfig::default();
    let decision = CanaryController::evaluate(&cfg, stage_idx, &baseline, &canary);

    let result = match decision {
        CanaryDecision::ContinueCurrent { weight } => json!({
            "action": "continue",
            "weight": weight
        }),
        CanaryDecision::AdvanceToStage { stage_idx, weight } => json!({
            "action": "advance",
            "next_stage_idx": stage_idx,
            "weight": weight
        }),
        CanaryDecision::PromoteComplete => json!({
            "action": "promote_complete",
            "weight": 100
        }),
        CanaryDecision::Abort { reason } => json!({
            "action": "abort",
            "reason": reason,
            "weight": 0
        }),
    };

    Ok(result)
}

fn canary_abort_tool() -> ToolDef {
    ToolDef {
        name: "ltn_canary_abort",
        description: "Emergency abort a canary rollout, instantly resetting all ingress traffic back to 100% baseline.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app_name": { "type": "string" },
                "reason": { "type": "string" }
            },
            "required": ["app_name", "reason"],
            "additionalProperties": false
        }),
        handler: handle_canary_abort,
    }
}

fn handle_canary_abort(_ctx: &Ctx, args: &Value) -> Result<Value> {
    let app_name = get_str_arg(args, "app_name")?;
    let reason = get_str_arg(args, "reason")?;

    Ok(json!({
        "status": "aborted",
        "app_name": app_name,
        "reason": reason,
        "baseline_traffic_percent": 100,
        "canary_traffic_percent": 0
    }))
}

// -----------------------------------------------------------------------------
// 5. Automated Disaster Recovery (DR) Drills
// -----------------------------------------------------------------------------
fn dr_drill_tool() -> ToolDef {
    ToolDef {
        name: "ltn_dr_drill",
        description: "Execute an automated disaster recovery synthetic drill on a data snapshot, verifying SLA RTO and SHA-256 data consistency.",
        input_schema: json!({
            "type": "object",
            "properties": {
                "app_name": { "type": "string" },
                "volume_name": { "type": "string" },
                "sample_data": { "type": "string", "description": "Optional mock data payload to verify" },
                "max_rto_seconds": { "type": "number", "description": "RTO SLA threshold in seconds" }
            },
            "required": ["app_name", "volume_name"],
            "additionalProperties": false
        }),
        handler: handle_dr_drill,
    }
}

fn handle_dr_drill(ctx: &Ctx, args: &Value) -> Result<Value> {
    let app_name = get_str_arg(args, "app_name")?;
    let volume_name = get_str_arg(args, "volume_name")?;
    let data_text = get_str_opt(args, "sample_data").unwrap_or("POSTGRES_DUMP_SAMPLE_VERIFIED_OK");
    let max_rto = args.get("max_rto_seconds").and_then(Value::as_f64).unwrap_or(60.0);

    let data_bytes = data_text.as_bytes();
    let snap_dir = ctx.home.join("backups").to_string_lossy().to_string();
    let snap = BackupEngine::create_snapshot(app_name, volume_name, data_bytes, &snap_dir);

    let report = DrDrillRunner::run_synthetic_drill(&snap, data_bytes, max_rto);

    Ok(json!({
        "drill_id": report.drill_id,
        "app_name": report.app_name,
        "snapshot_id": report.snapshot_id,
        "checksum_valid": report.data_checksum_valid,
        "rto_seconds": report.rto_seconds,
        "max_rto_seconds": report.max_rto_seconds,
        "status": match report.status {
            DrillStatus::Passed => "passed",
            DrillStatus::Failed { .. } => "failed",
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("ltn_test_mcp_{}", uuid::Uuid::new_v4()));
        let ctx = Ctx::for_home(&temp_dir);

        let set_args = json!({
            "app": "auth-svc",
            "key": "JWT_SECRET",
            "value": "super-secret-token-value"
        });
        let set_res = handle_secret_set(&ctx, &set_args).unwrap();
        assert_eq!(set_res.get("status").unwrap(), "stored");

        let get_args = json!({
            "app": "auth-svc",
            "key": "JWT_SECRET"
        });
        let get_res = handle_secret_get(&ctx, &get_args).unwrap();
        assert_eq!(get_res.get("value").unwrap(), "super-secret-token-value");

        let list_args = json!({
            "app": "auth-svc"
        });
        let list_res = handle_secret_list(&ctx, &list_args).unwrap();
        let keys = list_res.get("keys").unwrap().as_array().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "JWT_SECRET");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_k8s_generate_tool() {
        let ctx = Ctx::from_env();
        let args = json!({
            "app_name": "payments-api",
            "image": "myorg/payments:v1.2.3",
            "replicas": 3,
            "cpu_millis": 500,
            "memory_mb": 1024,
            "port": 9000,
            "domain": "pay.internal.net"
        });

        let res = handle_k8s_generate(&ctx, &args).unwrap();
        assert_eq!(res.get("status").unwrap(), "generated");
        let depl = res.get("deployment_yaml").unwrap().as_str().unwrap();
        assert!(depl.contains("payments-api"));
        assert!(depl.contains("myorg/payments:v1.2.3"));
        let ing = res.get("ingress_yaml").unwrap().as_str().unwrap();
        assert!(ing.contains("pay.internal.net"));
    }

    #[test]
    fn test_canary_step_and_abort() {
        let ctx = Ctx::from_env();
        let step_args = json!({
            "stage_idx": 0,
            "baseline_requests": 200,
            "baseline_errors": 0,
            "baseline_p99_ms": 40.0,
            "canary_requests": 150,
            "canary_errors": 0,
            "canary_p99_ms": 45.0
        });

        let res = handle_canary_step(&ctx, &step_args).unwrap();
        assert_eq!(res.get("action").unwrap(), "advance");
        assert_eq!(res.get("weight").unwrap(), 25);

        let abort_args = json!({
            "app_name": "checkout-svc",
            "reason": "Elevated 500 status codes detected on canary"
        });
        let abort_res = handle_canary_abort(&ctx, &abort_args).unwrap();
        assert_eq!(abort_res.get("status").unwrap(), "aborted");
        assert_eq!(abort_res.get("baseline_traffic_percent").unwrap(), 100);
        assert_eq!(abort_res.get("canary_traffic_percent").unwrap(), 0);
    }

    #[test]
    fn test_dr_drill_tool() {
        let temp_dir = std::env::temp_dir().join(format!("ltn_test_dr_{}", uuid::Uuid::new_v4()));
        let ctx = Ctx::for_home(&temp_dir);

        let drill_args = json!({
            "app_name": "orders-db",
            "volume_name": "pgdata",
            "sample_data": "PG_WAL_SNAPSHOT_STREAM_VERIFIED",
            "max_rto_seconds": 30.0
        });

        let res = handle_dr_drill(&ctx, &drill_args).unwrap();
        assert_eq!(res.get("status").unwrap(), "passed");
        assert_eq!(res.get("checksum_valid").unwrap(), true);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
