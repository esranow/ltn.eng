//! Orchestrator: targets, capacity, Z3 placement, plan/apply/rollback/destroy.

use anyhow::{bail, Result};
use ltn_state::{Ctx, ToolDef};
use serde_json::Value;

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    Vec::new()
}

/// Redeploy an earlier release of `app` (`to_release`, or the most recent
/// healthy one before the current). Used by the `ltn_rollback` tool and by the
/// watcher when a restart does not bring an app back.
pub fn rollback(_ctx: &Ctx, _app: &str, _to_release: Option<u32>) -> Result<Value> {
    bail!("ltn-engine rollback is not implemented yet")
}
