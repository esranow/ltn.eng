//! MCP stdio server exposing the ltn runtime tools.

use ltn_state::{Ctx, ToolDef};

/// Context with every provisioner registered.
pub fn context_from_env() -> Ctx {
    let mut ctx = Ctx::from_env();
    ctx.provisioners = ltn_cloud::provisioners();
    ctx
}

/// Every tool the server exposes.
pub fn all_tools() -> Vec<ToolDef> {
    let mut tools = ltn_engine::tools();
    tools.extend(ltn_observe::tools());
    tools.extend(ltn_cloud::tools());
    tools
}
