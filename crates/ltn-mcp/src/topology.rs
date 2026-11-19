//! `map_mtl_topology`: this machine's hardware, from map-mtl when installed.

use crate::find_binary;
use anyhow::{anyhow, bail, Context, Result};
use ltn_core::topology::HostHardwareInventory;
use ltn_state::{Ctx, ToolDef};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SCAN_TIMEOUT: Duration = Duration::from_secs(120);

pub fn tool() -> ToolDef {
    ToolDef {
        name: "map_mtl_topology",
        description: "Hardware topology of the machine running the server. Runs `map-mtl scan --json` \
                      (binary from LTN_MAP_MTL_BIN or PATH) and returns its JSON untouched with \
                      source \"map-mtl\"; if map-mtl is not installed, returns the built-in sysinfo \
                      probe with source \"builtin-probe\".",
        input_schema: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        handler: run,
    }
}

pub fn run(_ctx: &Ctx, _args: &Value) -> Result<Value> {
    let explicit = std::env::var_os("LTN_MAP_MTL_BIN").filter(|v| !v.is_empty()).is_some();
    match find_binary("LTN_MAP_MTL_BIN", "map-mtl") {
        Ok(bin) => {
            let topology = scan(&bin)?;
            Ok(json!({ "source": "map-mtl", "binary": bin.display().to_string(), "topology": topology }))
        }
        // An explicit override that points nowhere is a configuration error, not a reason to guess.
        Err(e) if explicit => bail!("{e}. Fix LTN_MAP_MTL_BIN or unset it to use the built-in probe."),
        Err(_) => {
            let inv = HostHardwareInventory::probe_local_system();
            Ok(json!({ "source": "builtin-probe", "topology": serde_json::to_value(inv)? }))
        }
    }
}

/// Run `<bin> scan --json` and parse stdout. Errors carry its stderr.
fn scan(bin: &Path) -> Result<Value> {
    let mut child = Command::new(bin)
        .args(["scan", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {}", bin.display()))?;
    let mut out_pipe = child.stdout.take().expect("piped stdout");
    let mut err_pipe = child.stderr.take().expect("piped stderr");
    let out_reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out_pipe.read_to_end(&mut s);
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err_pipe.read_to_end(&mut s);
        s
    });

    let deadline = Instant::now() + SCAN_TIMEOUT;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).trim().to_string();
    let stderr_tail = ltn_state::docker::tail_lines(&stderr, 20);

    let Some(status) = status else {
        bail!("`{} scan --json` timed out after {}s; stderr: {stderr_tail}", bin.display(), SCAN_TIMEOUT.as_secs());
    };
    if !status.success() {
        bail!("`{} scan --json` failed ({status}); stderr: {stderr_tail}", bin.display());
    }
    serde_json::from_slice(&stdout).map_err(|e| {
        anyhow!(
            "`{} scan --json` printed invalid JSON ({e}); stderr: {stderr_tail}; stdout starts: {}",
            bin.display(),
            String::from_utf8_lossy(&stdout).chars().take(200).collect::<String>()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_of_failing_binary_reports_error() {
        // `cmd /C exit 1`-style failure: use a binary that exists but rejects `scan --json`.
        let bin = if cfg!(windows) { Path::new("C:\\Windows\\System32\\whoami.exe") } else { Path::new("/bin/false") };
        if !bin.is_file() {
            return;
        }
        let err = scan(bin).unwrap_err().to_string();
        assert!(err.contains("scan --json"), "{err}");
    }
}
