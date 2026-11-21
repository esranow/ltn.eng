//! MCP stdio server exposing the ltn runtime tools.
//!
//! JSON-RPC 2.0 over stdio, one message per line. Only protocol messages are
//! written to stdout; every diagnostic goes to stderr.

mod doctor;
mod ops_tools;
mod server;
mod topology;

pub use server::{handle_message, serve_stdio, PROTOCOL_VERSIONS};

use ltn_state::{Ctx, ToolDef};

/// Context with every provisioner registered.
pub fn context_from_env() -> Ctx {
    let mut ctx = Ctx::from_env();
    ctx.provisioners = ltn_cloud::provisioners();
    ctx
}

/// Tools owned by this crate: `ltn_doctor`, `map_mtl_topology`, and autonomous ops tools.
pub fn tools() -> Vec<ToolDef> {
    let mut list = vec![doctor::tool(), topology::tool()];
    list.extend(ops_tools::tools());
    list
}

/// Every tool the server exposes.
pub fn all_tools() -> Vec<ToolDef> {
    let mut tools = ltn_engine::tools();
    tools.extend(ltn_observe::tools());
    tools.extend(ltn_cloud::tools());
    tools.extend(self::tools());
    tools
}

/// Resolve a binary: the env override if set (it must exist), otherwise the
/// first match for `name` on PATH. `Err` carries why nothing usable was found.
pub(crate) fn find_binary(env_var: &str, name: &str) -> Result<std::path::PathBuf, String> {
    if let Some(v) = std::env::var_os(env_var).filter(|v| !v.is_empty()) {
        let p = std::path::PathBuf::from(&v);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("{env_var}={} does not point to a file", p.display()))
        };
    }
    find_on_path(name).ok_or_else(|| format!("`{name}` not found on PATH (and {env_var} is not set)"))
}

/// First file named `name` (with any PATHEXT extension on Windows) in PATH.
pub(crate) fn find_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        let mut v = vec![String::new()];
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        v.extend(pathext.split(';').filter(|s| !s.is_empty()).map(|s| s.to_ascii_lowercase()));
        v
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
