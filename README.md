# ltn

ltn is an MCP server that lets an AI agent deploy, observe, debug and roll back
containerised apps on Docker hosts. A host can be the local Docker daemon, an
on-prem machine reached over SSH (`docker -H ssh://user@host`), or a cloud VM
that ltn provisions through OpenTofu and then registers like any other host.
One deploy, observe and debug path serves all three.

The agent works through tools; there is no dashboard. Placement is a real Z3
query over the capacity each host reports. Status, logs and diagnosis come
from the container runtime. When something is missing (Docker, SSH access,
credentials, a binary), the tool returns an error that says so. It never
reports a simulated success.

## Tools

| Tool | What it does |
|---|---|
| `ltn_doctor` | Checks the environment: Docker reachable (version, CPUs, memory), Z3 linked, state dir writable, `ssh`/OpenTofu/map-mtl found, provisioner offers, recorded targets and deployments |
| `map_mtl_topology` | This machine's hardware: output of `map-mtl scan --json` if installed, otherwise a built-in sysinfo probe (the `source` field says which) |
| `ltn_analyze` | Reads a project's Dockerfile and optional `ltn.toml` into an app spec |
| `ltn_plan` | Chooses a target with Z3 under CPU, RAM, GPU and budget constraints; returns a `plan_id` |
| `ltn_apply` | Builds the image and runs a new release from a `plan_id`, gated on a health check |
| `ltn_rollback` | Returns an app to an earlier release |
| `ltn_destroy` | Removes an app's containers (needs `confirm` equal to the app name) |
| `ltn_targets` / `ltn_target_add` / `ltn_target_remove` | Lists, adds (SSH Docker hosts) and removes targets |
| `ltn_status` | Per-app verdict (`healthy`, `degraded`, `down`, `missing`) from `docker inspect`, an HTTP probe and stats |
| `ltn_logs` | Container logs for an app's current or a given release |
| `ltn_diagnose` | Findings with severity, cause, evidence and a suggested action |
| `ltn_repair` | Applies the safe repair for the current problem (restart or start) |
| `ltn_audit` | The audit log of state-changing actions, including the watcher's |
| `ltn_cloud_auth` / `ltn_cloud_catalog` / `ltn_cloud_targets` | Cloud credential check, instance catalog, and cloud-provisioned targets (AWS) |

`tools/list` on a running server is the source of truth for names, arguments
and descriptions.

## Quickstart

Building needs Z3 4.12 (headers and DLL). With bash on Windows:

```bash
export Z3_SYS_Z3_HEADER='D:\path\to\z3-4.12.2-x64-win\include\z3.h'
export BINDGEN_EXTRA_CLANG_ARGS='-ID:\path\to\z3-4.12.2-x64-win\include'
export Z3_DIR='D:\path\to\z3-4.12.2-x64-win'
export PATH="/d/path/to/z3-4.12.2-x64-win/bin:$PATH"   # libz3.dll must be on PATH at run time too
cargo build -p ltn-cli
```

Then run the server over stdio:

```bash
target/debug/ltn mcp
```

It speaks MCP (JSON-RPC 2.0, one message per line) on stdin and stdout and
writes diagnostics to stderr. To add it to an MCP client, see
[`examples/claude_mcp.json`](examples/claude_mcp.json); for a Google ADK agent,
see [`examples/adk_agent.py`](examples/adk_agent.py).

A typical session: `ltn_doctor`, `ltn_analyze {path}`, `ltn_plan {path}`,
`ltn_apply {plan_id}`, `ltn_status`, `ltn_logs {app}`. When something breaks,
use `ltn_diagnose {app}`, then `ltn_repair {app}` or `ltn_rollback {app}`.
`ltn_destroy {app, confirm: app}` removes the app.

## Targets

The local Docker daemon is always the target `local`. Add an on-prem or
remote machine with `ltn_target_add {id, docker_host: "ssh://user@host",
address}`. It must be reachable with your SSH agent or key files, and the
remote user must be able to run `docker`. ltn checks it with `docker info`
before recording it. Every target's free capacity goes into the Z3 placement.

Cloud capacity comes from provisioners (AWS through OpenTofu). Their offers
appear in placement next to existing targets. When a plan picks an offer,
`ltn_apply` first creates the VM, which must be allowed with
`LTN_ALLOW_CLOUD_SPEND=1`, and then registers it as a target. Credentials come
from the provider's standard credential chain in the server's environment. No
tool accepts a secret as an argument.

## `ltn.toml`

A project needs a `Dockerfile`. An optional `ltn.toml` next to it overrides
what ltn reads or defaults. Unknown keys are rejected.

```toml
name = "web"              # default: directory name
dockerfile = "Dockerfile"
port = 8080               # container port; default: the Dockerfile's EXPOSE
host_port = 18080         # published port on the target; default: port
health_path = "/"
health_timeout_s = 60
cpu = 0.5                 # cores
ram_mb = 512
gpu = 0
vram_mb = 0
max_cost_cents_per_hour = 20   # default: no cap
[env]
LOG_LEVEL = "info"
```

`ltn_analyze` reports where each value came from (`sources`).

## Guardrails and environment

Enforced in code:

- `ltn_apply` only accepts an unapplied `plan_id` from `ltn_plan` that is less than 30 minutes old.
- `ltn_destroy` needs `confirm` equal to the app name. Removing a cloud target needs `confirm` equal to its id.
- A release counts as healthy only if its container is running, has not restarted during the health gate, and answers HTTP 2xx/3xx on its port. A failing release is not promoted, and the previous one keeps serving.

| Variable | Effect |
|---|---|
| `LTN_HOME` | State directory (`state.json`, `audit.jsonl`); default `~/.ltn/runtime` |
| `LTN_MAX_HOURLY_CENTS` | Budget cap in the Z3 query; the lower of this and the app's `max_cost_cents_per_hour` applies |
| `LTN_ALLOW_CLOUD_SPEND` | Must be `1` before anything that creates billable cloud resources runs |
| `LTN_WATCH` | `0` disables the background watcher (on by default with `ltn mcp`) |
| `LTN_WATCH_INTERVAL_S` | Watcher interval in seconds; default 15 |
| `LTN_DOCKER_BIN` | Docker CLI to use; default `docker` |
| `LTN_TOFU_BIN`, `LTN_MAP_MTL_BIN` | Paths to OpenTofu and map-mtl if they are not on `PATH` |

The watcher checks every deployment on each interval. It restarts or starts a
container that is down and calls rollback when restarts do not help. Each
action is written to the audit log as `watch.*`. It skips an app while an
apply or rollback holds that app's lock.

## Status and limits

- Tested against a real local Docker daemon (Docker Desktop on Windows):
  `tests/e2e/mcp_e2e.py` drives `ltn mcp` over stdio through doctor, analyze,
  plan, apply, HTTP check, status, logs, a second release, `docker kill` with
  watcher recovery, a crashing release being rejected, diagnose, rollback,
  destroy, and apply failing when the Docker binary is missing. Run it with
  `python tests/e2e/mcp_e2e.py [path/to/ltn.exe]` after building. It uses host
  ports 18300-18399.
- The protocol layer and `ltn_doctor` have unit tests in `crates/ltn-mcp`.
  Tests that need Docker are marked `#[ignore]`; run them with
  `cargo test -p <crate> -- --ignored --test-threads=1`.
- SSH targets use the same Docker CLI path as local ones. They are not covered
  by the end-to-end script.
- The AWS path generates OpenTofu and refuses to spend without
  `LTN_ALLOW_CLOUD_SPEND=1`. It has not been run against a live AWS account.
- `examples/adk_agent.py` has not been run.
- The older crates (`ltn-architect`, `ltn-agent`, `ltn-providers`, `ltn-ops`,
  `ltn-container`, most of `ltn-core`) and the other `ltn` CLI subcommands
  are earlier demos. They are not part of the MCP surface. The previous MCP
  server is still reachable as the hidden `ltn mcp-legacy`. Some
  `ltn-providers` tests need a MiniStack mock on `127.0.0.1:4566` and are
  ignored by default.
