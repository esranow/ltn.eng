# Sprint plan: make the ltn MCP server real

Branch `sprint/real-deploy-path`. Five agents work in parallel on disjoint files; the coordinator integrates and commits.

## Goal

A customer adds the ltn MCP server to their agent kit (Google ADK, Claude Agent SDK, any MCP client), supplies cloud credentials or an SSH-reachable machine, and the agent does the DevOps work through tools. No dashboard. The server plays four roles:

- **Architect**: read the project, size it, choose where it runs with a real Z3 query.
- **Orchestrator**: build, deploy, roll back, destroy.
- **Observer**: report what is actually running, from the container runtime.
- **Debugger**: explain failures from real evidence, repair what is safe to repair.

`map-mtl` is the open-source hardware inventory tool that feeds on-prem capacity.

## The one rule

**Never report success for something that did not happen.** No fabricated IDs, no "simulated" fallbacks, no hardcoded metrics, no default-healthy. If Docker, SSH, credentials or a binary is missing, return an error that says so. A test that passes without the real thing running must be a unit test of pure logic, not a claim about behaviour.

## Architecture

Everything deployable is **a Docker host**. Local machine: the local daemon. On-prem: `docker -H ssh://user@host`. Cloud: OpenTofu creates a VM with Docker and SSH, which is then registered as a target like any other. One deploy, observe and debug path serves all three.

```
ltn-cli (`ltn mcp`)
  └─ ltn-mcp      stdio JSON-RPC server, wires everything          (Agent 3)
       ├─ ltn-engine   targets, capacity, Z3 placement, plan/apply  (Agent 1)
       ├─ ltn-observe  status, logs, diagnose, watcher/auto-repair  (Agent 2)
       └─ ltn-cloud    catalog + OpenTofu provisioner (AWS)         (Agent 4)
            all three depend only on:
       ltn-state   FROZEN: spec, docker wrapper, store, contracts   (coordinator)
       ltn-core::solver::placement   FROZEN: Z3 placement           (coordinator)
map-mtl (D:\ai_projects\map-mtl)  separate tool                     (Agent 5)
```

## Frozen contract (read these files first)

- `crates/ltn-state/src/tool.rs`: `Ctx` (state dir, docker binary, provisioners), `ToolDef { name, description, input_schema, handler: fn(&Ctx, &Value) -> Result<Value> }`, `Offer`, `trait Provisioner`, argument helpers.
- `crates/ltn-state/src/store.rs`: `State { targets, deployments, plans, ext }`, `Target`, `Plan`, `Deployment`, `Release`, `container_name(app, release)`, `Store::{load, save, update, audit, recent_audit}`.
- `crates/ltn-state/src/spec.rs`: `AppSpec`, `analyze(path)`: reads the project's Dockerfile and optional `ltn.toml`.
- `crates/ltn-state/src/docker.rs`: `Docker::{run, run_ok, info, inspect, logs, stats, image_exists, containers_with_label}`, `ContainerState::from_inspect`.
- `crates/ltn-core/src/solver/placement.rs`: `place(&PlacementRequest, &[TargetCapacity]) -> Result<PlacementDecision, PlacementError>`.

Do not edit frozen files. If something is missing, add a helper in your own crate and say so in your report.

Each crate exposes `pub fn tools() -> Vec<ToolDef>`; the skeleton `lib.rs` in each crate already has the exact public signatures the others rely on. Keep those signatures.

### Conventions

- **Container naming and labels.** Containers are named `container_name(app, release)` and carry labels `ltn.app=<app>` and `ltn.release=<n>`. Images are tagged `ltn/<app>:r<n>`.
- **State updates.** Never hold `Store::update` across docker or network calls. Work on a `load()` snapshot, then record the result with a short `update`.
- **Audit.** Every state-changing action calls `store.audit(app, action, detail)`.
- **Tool results.** Handlers return a JSON object. Failures return `Err` with a message an agent can act on (what failed, the evidence, what to do next).
- **Secrets.** No tool takes a secret as an argument. Cloud credentials come from the provider's standard credential chain in the server's environment; SSH uses the user's agent or key files.
- **Extension data.** A crate that needs to persist its own records uses `State::ext["<crate>"]`.

- **Restart policy.** App containers run with `--restart unless-stopped`, `--cpus` and `--memory` from the spec. A release is healthy only if the container is running, not restarting, has `RestartCount == 0` during the apply health gate, and (when the spec has a port) `GET http://<target.address>:<host_port><health_path>` returns 2xx or 3xx before `health_timeout_s`.
- **Busy lock.** While the engine is applying or rolling back an app it sets `State.ext["locks"][<app>]` to an RFC 3339 expiry time (now + 10 minutes) and removes it when done. The watcher skips any app with an unexpired lock.
- **Docker-dependent tests.** Mark them `#[ignore = "requires a running Docker daemon"]` and run them with `cargo test -p <crate> -- --ignored --test-threads=1`. Plain `cargo test` must pass with no Docker.

## Tool arguments and key result fields

All three sides (engine, observe, the MCP end-to-end script) rely on these names.

| Tool | Arguments | Result must include |
|---|---|---|
| `ltn_analyze` | `path` | the `AppSpec` fields |
| `ltn_plan` | `path`, `target?` (pin to one target id) | `plan_id`, `app`, `target_id`, `provision_offer`, `cost_cents_per_hour`, `capacity_considered`, `expires_at` |
| `ltn_apply` | `plan_id` | `app`, `release`, `target_id`, `container`, `endpoint` (URL or null), `health` |
| `ltn_rollback` | `app`, `to_release?` | `app`, `release` (now active), `from_release`, `health` |
| `ltn_destroy` | `app`, `confirm` | `app`, `removed_containers` |
| `ltn_targets` | none | `targets: [{id, provider, docker_host, reachable, capacity?, error?}]` |
| `ltn_target_add` | `id`, `docker_host`, `address`, `cost_cents_per_hour?`, `gpu_count?`, `vram_mb?` | `id`, `reachable: true`, `capacity` |
| `ltn_target_remove` | `id`, `confirm?` | `id`, `removed: true` |
| `ltn_status` | `app?` | `apps: [{app, verdict, release, target_id, container: {...}, probe: {...}, stats: {...}}]`, verdict one of `healthy`, `degraded`, `down`, `missing` |
| `ltn_logs` | `app`, `tail?` (default 100), `release?` | `app`, `release`, `container`, `logs` |
| `ltn_diagnose` | `app` | `app`, `verdict`, `findings: [{severity, cause, evidence, suggested_action}]` |
| `ltn_repair` | `app` | `app`, `action`, `before`, `after` |
| `ltn_audit` | `app?`, `limit?` (default 50) | `entries` |

## Tool ownership

| Crate | Tools |
|---|---|
| ltn-engine | `ltn_analyze`, `ltn_plan`, `ltn_apply`, `ltn_rollback`, `ltn_destroy`, `ltn_targets`, `ltn_target_add`, `ltn_target_remove` |
| ltn-observe | `ltn_status`, `ltn_logs`, `ltn_diagnose`, `ltn_repair`, `ltn_audit` |
| ltn-cloud | `ltn_cloud_auth`, `ltn_cloud_catalog`, `ltn_cloud_targets` |
| ltn-mcp | `ltn_doctor`, `map_mtl_topology` |

## Guardrails (must be enforced in code, not in prompts)

- `ltn_apply` only accepts a `plan_id` returned by `ltn_plan`, unapplied and less than 30 minutes old.
- Budget: the spec's `max_cost_cents_per_hour` and the env cap `LTN_MAX_HOURLY_CENTS` both go into the Z3 query as the budget constraint.
- `ltn_destroy` requires `confirm` equal to the app name. Removing a cloud target requires `confirm` equal to the target id.
- Anything that creates billable cloud resources is refused unless `LTN_ALLOW_CLOUD_SPEND=1` is set in the server's environment.

## Working rules for agents

- **Files.** Edit only the paths your brief assigns. Other crates are being edited by other agents right now.
- **Builds.** Set the Z3 environment before cargo (see below). The cargo target directory is shared, so builds queue; that is expected. If a crate that is not yours fails to compile, it is mid-edit: wait a minute and retry, do not fix it.
- **Keep your crate compiling.** Run `cargo check -p <your crate>` before you stop editing for any reason.
- **Git.** Do not run any git command that writes (add, commit, stash, checkout, reset). The coordinator commits.
- **Docker tests.** Real Docker is running locally (8 CPUs, 8 GB). Use a unique app name per test (uuid suffix), a temp `LTN_HOME`/`Ctx::for_home`, host ports only inside your assigned range, tiny images (`busybox`, `alpine`), and remove your containers and images at the end of each test even on failure.
- **Dependencies.** Add new dependencies only in your own crate's `Cargo.toml`, with explicit versions.
- **Time.** You have 75 minutes. A smaller scope that is finished and tested beats a larger one that is not. Stop and report at the limit.
- **Report.** End with: what works and is tested (with exact test counts), what is written but untested and why, what you did not get to, and anything another crate must do to use yours.

### Z3 environment (bash)

```bash
export Z3_SYS_Z3_HEADER='D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h'
export BINDGEN_EXTRA_CLANG_ARGS='-ID:\beam\z3-bin\z3-4.12.2-x64-win\include'
export Z3_DIR='D:\beam\z3-bin\z3-4.12.2-x64-win'
export PATH="/d/beam/z3-bin/z3-4.12.2-x64-win/bin:$PATH"
```

## Timeline (3 hours)

| Window | Work |
|---|---|
| 0:00–0:35 | Coordinator: branch, snapshot commit, Z3 placement, frozen base, this plan (done) |
| 0:35–1:55 | Five agents in parallel |
| 1:55–2:40 | Coordinator: integrate, wire watcher and provisioners, full end-to-end run over MCP stdio |
| 2:40–3:00 | Fix what the end-to-end run finds, full test suite, commit, report |

## Done means

1. `ltn mcp` lists only real tools, and an MCP client can: analyze → plan (Z3) → apply → status → logs → diagnose → rollback → destroy, against real Docker.
2. Killing a deployed container is detected and repaired by the watcher; a broken release is rolled back automatically.
3. With Docker unavailable, apply returns an error.
4. A second target can be added by SSH docker host and is considered by the Z3 placement.
5. The AWS path generates valid OpenTofu and refuses to spend without the explicit env switch.
6. `map-mtl scan --json` compiles and prints this machine's real hardware.
7. `cargo test --workspace` is green; tests needing external services are gated and say so.
