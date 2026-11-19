//! Cloud provisioner: instance catalog and OpenTofu-provisioned Docker hosts.

use ltn_state::{Provisioner, ToolDef};
use std::sync::Arc;

/// MCP tools owned by this crate.
pub fn tools() -> Vec<ToolDef> {
    Vec::new()
}

/// Provisioners to register in `Ctx::provisioners`.
pub fn provisioners() -> Vec<Arc<dyn Provisioner>> {
    Vec::new()
}
