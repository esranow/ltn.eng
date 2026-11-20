//! Orchestrator: targets, capacity, Z3 placement, plan/apply/rollback/destroy.

pub mod capacity;
pub mod deploy;
pub mod plan;
pub mod targets;

use anyhow::Result;
use ltn_state::{Ctx, ToolDef};
use serde_json::{json, Value};

pub use deploy::{is_locked, BusyLock, LOCKS_KEY};

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ltn_analyze",
            description: "Read a project directory (its Dockerfile and optional ltn.toml) and report what it needs: \
                          name, port, host_port, health path, cpu_millis, ram_mb, GPUs, budget, env, and where each value came from.",
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Project directory containing a Dockerfile" } },
                "required": ["path"],
            }),
            handler: plan::analyze_tool,
        },
        ToolDef {
            name: "ltn_plan",
            description: "Choose where a project runs with a Z3 placement query over the free capacity of every reachable \
                          target plus cloud offers, under the budget (ltn.toml max_cost_cents_per_hour and LTN_MAX_HOURLY_CENTS). \
                          Returns a plan_id valid for 30 minutes; pass it to ltn_apply. If nothing fits, the error names the \
                          conflicting constraints and each candidate's free capacity.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Project directory containing a Dockerfile" },
                    "target": { "type": "string", "description": "Optional: only consider this target id (or offer id)" },
                },
                "required": ["path"],
            }),
            handler: plan::plan_tool,
        },
        ToolDef {
            name: "ltn_apply",
            description: "Execute a plan: provision capacity if the plan needs it, docker build the image on the target, \
                          replace the current release and health-gate the new container. On failure the previous release is \
                          restored and the error carries the exit code, OOM flag and last 50 log lines.",
            input_schema: json!({
                "type": "object",
                "properties": { "plan_id": { "type": "string", "description": "plan_id returned by ltn_plan" } },
                "required": ["plan_id"],
            }),
            handler: deploy::apply,
        },
        ToolDef {
            name: "ltn_rollback",
            description: "Redeploy an earlier release of an app from its kept image (default: the most recent superseded \
                          release), with the same health gate and automatic restore as ltn_apply.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": { "type": "string" },
                    "to_release": { "type": "integer", "minimum": 1, "description": "Release number to return to" },
                },
                "required": ["app"],
            }),
            handler: deploy::rollback_tool,
        },
        ToolDef {
            name: "ltn_destroy",
            description: "Remove every container and image of an app from its target and delete its deployment record. \
                          Requires confirm equal to the app name.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app": { "type": "string" },
                    "confirm": { "type": "string", "description": "Must equal the app name" },
                },
                "required": ["app", "confirm"],
            }),
            handler: deploy::destroy,
        },
        ToolDef {
            name: "ltn_targets",
            description: "List deployment targets (Docker hosts) with reachability and free capacity (docker info minus \
                          what current releases reserve).",
            input_schema: json!({ "type": "object", "properties": {} }),
            handler: targets::list,
        },
        ToolDef {
            name: "ltn_target_add",
            description: "Register a Docker host reachable over SSH (docker_host like ssh://deploy@10.0.0.5) as a target. \
                          The host is verified with `docker info` before it is saved.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Lowercase id, e.g. edge-1" },
                    "docker_host": { "type": "string", "description": "e.g. ssh://deploy@10.0.0.5 or ssh://deploy@10.0.0.5:2222" },
                    "identity_file": { "type": "string", "description": "Path on the ltn server to the private key for this host (a path, never key material). Written to ltn's own ssh config; ~/.ssh is not used or changed." },
                    "address":{ "type": "string", "description": "Host or IP used to reach published ports" },
                    "cost_cents_per_hour": { "type": "integer", "minimum": 0 },
                    "gpu_count": { "type": "integer", "minimum": 0 },
                    "vram_mb": { "type": "integer", "minimum": 0 },
                },
                "required": ["id", "docker_host", "address"],
            }),
            handler: targets::add,
        },
        ToolDef {
            name: "ltn_target_remove",
            description: "Unregister a target. Refused while any deployment lives on it. Cloud targets are deprovisioned \
                          and need confirm equal to the target id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "confirm": { "type": "string", "description": "Target id, required for cloud targets" },
                },
                "required": ["id"],
            }),
            handler: targets::remove,
        },
    ]
}

/// Redeploy an earlier release of `app` (`to_release`, or the most recent
/// superseded one before the current). Used by the `ltn_rollback` tool and by
/// the watcher when a restart does not bring an app back.
pub fn rollback(ctx: &Ctx, app: &str, to_release: Option<u32>) -> Result<Value> {
    deploy::rollback(ctx, app, to_release)
}

#[cfg(test)]
mod tests;
