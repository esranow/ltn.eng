//! Observer and debugger: status, logs, diagnosis, and the watch loop.

use anyhow::Result;
use ltn_state::{Ctx, ToolDef};
use serde_json::Value;

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    Vec::new()
}

/// What the watcher calls when restarting an app did not make it healthy.
/// Matches `ltn_engine::rollback`.
pub type RollbackHook = fn(&Ctx, &str, Option<u32>) -> Result<Value>;

/// Start the background watch loop: every `interval` it checks each
/// deployment, restarts what is down, and calls `rollback` when restarts fail.
pub fn spawn_watcher(
    _ctx: Ctx,
    _interval: std::time::Duration,
    _rollback: RollbackHook,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {})
}
