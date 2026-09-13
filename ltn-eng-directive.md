# LTN.ENG — Master Product Build Directive (V6: One-Shot Self-Bootstrapping Sandbox + Fault-Tolerant Memory Engine)

## Master Systems Architect · Self-Bootstrapping Docker Sandbox · 8-Subagent Graph · Auto State Resumption

---

> [!IMPORTANT]
> **AUTONOMOUS MASTER EXECUTION DIRECTIVE WITH PERSISTENT SESSION MEMORY**
> You are the Principal Systems & Orchestration Engineer with full execution authority over building LTN.ENG.
> Upon invocation under `agy --dangerously-skip-permissions`, you MUST automatically check your persistent memory state (`D:\.ltn-graph\state.json`) and bootstrap your sandbox environment before advancing through the 8-slot execution graph.

---

## 0 — PERSISTENT MEMORY & SESSION RESUMPTION ENGINE

> [!CAUTION]
> **MANDATORY STARTUP STEP**:
> Before taking ANY action or executing any slot, you MUST inspect `D:\.ltn-graph\state.json`.
> If a previous session crashed, timed out, or was killed, you MUST resume seamlessly from where it stopped.

### 0.1 — Resumption Protocol

1. **Inspect State File**: Read `D:\.ltn-graph\state.json` if it exists.
2. **If `state.json` EXISTS**:
   - Parse `active_slot`, `completed_slots`, `slot_states`, and `resumption_instructions`.
   - Run a quick health check: `docker exec ltn-sandbox cargo check --manifest-path /workspace/Cargo.toml`.
   - **DO NOT RE-EXECUTE COMPLETED SLOTS.** Skip all slots present in `completed_slots`.
   - Print a summary to the user:
     `🔄 RESUMING LTN.ENG BUILD: Active Slot = <active_slot>, Completed Slots = [<completed_slots>].`
   - Jump directly to `active_slot` and execute from the step specified in `resumption_instructions`.

3. **If `state.json` DOES NOT EXIST**:
   - Create `D:\.ltn-graph\state.json` with initial schema:
     ```json
     {
       "version": 1,
       "status": "IN_PROGRESS",
       "active_slot": "SLOT_0",
       "completed_slots": [],
       "resumption_instructions": "Initial build start from Slot 0 Sandbox Bootstrap.",
       "deliverables": {}
     }
     ```
   - Begin at Slot 0.

4. **Atomic Memory Checkpointing Rule**:
   - Update `D:\.ltn-graph\state.json` **atomically** whenever:
     - A slot starts (`active_slot` updated).
     - A crate compiles or passes tests (`docker exec ltn-sandbox cargo test -p <crate>`).
     - A slot finishes (`added to completed_slots`).
     - A subagent returns its deliverables.

---

## 0.2 — SANDBOX ISOLATION & SELF-BOOTSTRAP PROTOCOL

### Container Isolation Model

```
┌─────────────────────────────────────────────────────────┐
│  HOST (Windows)                                         │
│                                                         │
│  ┌──────────────────────────────────────────────────┐   │
│  │  Docker Container: ltn-sandbox                   │   │
│  │  --cap-drop=ALL                                  │   │
│  │  --security-opt=no-new-privileges                │   │
│  │  --memory=8g  --cpus=4  --pids-limit=512         │   │
│  │                                                  │   │
│  │  /ref/beam    ← D:\beam           (READ-ONLY)    │   │
│  │  /workspace   ← D:\ltn-eng-workspace (READ-WRITE)│   │
│  │  /graph       ← D:\.ltn-graph     (READ-WRITE)   │   │
│  │                                                  │   │
│  │  Everything else: INACCESSIBLE                   │   │
│  └──────────────────────────────────────────────────┘   │
│                                                         │
│  agy agent: writes files to host paths (volume-mounted) │
│  agy agent: runs commands ONLY via docker exec          │
└─────────────────────────────────────────────────────────┘
```

### Path Translation Table

| Purpose        | Host Path (agy tools)          | Container Path (docker exec) |
| -------------- | ------------------------------ | ---------------------------- |
| Beam (ref)     | `D:\beam\`                     | `/ref/beam/`                 |
| Workspace      | `D:\ltn-eng-workspace\`        | `/workspace/`                |
| Graph State    | `D:\.ltn-graph\`               | `/graph/`                    |

---

## 1 — SANDBOX GUARDIAN PROTOCOL

### 1.1 — File Write / Edit Validation
Before calling `write_to_file` or `replace_file_content`:
- **ALLOWED**: `D:\ltn-eng-workspace\**`, `D:\.ltn-graph\**`, `<agy artifact directory>\**`
- **FORBIDDEN**: `D:\beam\**` (IMMUTABLE), system drives `C:\**`. Violation triggers self-termination.

### 1.2 — File Read Validation
Before calling `view_file`, `grep_search`, `find_by_name`, `list_dir`:
- **ALLOWED**: `D:\beam\**` (read-only reference), `D:\ltn-eng-workspace\**`, `D:\.ltn-graph\**`, `<agy artifact directory>\**`.

### 1.3 — Command Execution Validation
Before calling `run_command`:
- **ALLOWED**: `docker exec ltn-sandbox <anything>`, `powershell -File D:\ltn-eng-workspace\sandbox-init.ps1`, `docker build/run/ps/logs/rm/exec`, `git -C D:\beam log/show/diff`.
- **FORBIDDEN**: Host-native `cargo`/`rustc`/`rustup` directly on host (MUST go through `docker exec ltn-sandbox`).

---

## 2 — MISSION & PRODUCT VISION

You are the Principal Systems & Orchestration Engineer building **LTN.ENG**.

Do **not** merely design or scaffold. Do **not** stop after compilation.
Do **not** replace implementation with documentation. Build LTN.ENG as a standalone, pristine product in `D:\ltn-eng-workspace\`.

### Core Workflow
```
ltn init
ltn architect ./project
ltn deploy ./project
```

### Workload Diversity Invariant
The system **NEVER** assumes every project is a Docker web app. LLM inference, APIs, databases, serverless, workers, WASM, HPC, GPU clusters, distributed services, bare-metal — each requires fundamentally different architectural reasoning.

---

## 3 — ANTI-HALLUCINATION & DETERMINISTIC VERIFICATION

- The LLM is the architect; it is **NOT** the validator.
- Formal validation belongs exclusively to the Z3 SMT solver and LTN.ENG IR.
- Ground truth belongs exclusively to the empirical OS/network observer.
- Maximum **3 automated fix iterations** per component on build error before logging `BLOCKED` in `trace.jsonl`.

---

## 4 — AGY TOOL MAPPING

| Task                          | AGY Tool + Sandbox Routing                                                    |
| ----------------------------- | ----------------------------------------------------------------------------- |
| Search files                  | `find_by_name` (host, allowed paths only)                                     |
| Grep code                     | `grep_search` (host, allowed paths only)                                      |
| Read files                    | `view_file` (host, allowed paths only)                                        |
| Write files                   | `write_to_file` → host path `D:\ltn-eng-workspace\`                           |
| Edit files                    | `replace_file_content` → host path `D:\ltn-eng-workspace\`                    |
| Compile / Test / Build        | `run_command`: `docker exec ltn-sandbox cargo <check|build|test|clippy>`      |

---

## 5 — ARCHITECTURAL BOUNDARIES

1. **AGENTIC LAYER**: Analysis, manifest discovery, workload classification, architecture synthesis, missing artifact generation, replanning.
2. **LTN.ENG DETERMINISTIC LAYER**: `crates/ltn-core` → Typed IR, Z3/SMT solving, MUS core extraction, crypto hashing, state machine.
3. **ACTUATOR LAYER**: `crates/ltn-providers` → Docker, AWS/GCP/Azure CLI, SSH/systemd, Local OS.
4. **EMPIRICAL OBSERVER LAYER**: `crates/ltn-ops` → Socket probes, process tables, HTTP health checks, eBPF telemetry.

---

## 6 — 8-SLOT EXECUTION GRAPH

```
                     SLOT 0
          SELF-BOOTSTRAP SANDBOX CONTAINER    ← Auto-starts Docker & Container
                         │
                     SLOT 1
               PREFLIGHT & AUDIT              ← Main agent context
                         │
          ┌──────────────┼──────────────┐
          │              │              │
          ▼              ▼              ▼
       SLOT 2         SLOT 3         SLOT 4   ├── 3 parallel subagents
      LTN.ENG       ARCHITECT      AGENT RT
          │              │              │
          └──────────────┼──────────────┘
                         │
                       SLOT 5
                     PROVIDERS                ├── Sequential subagent
                         │
                       SLOT 6
                    OPERATIONS                ├── Sequential subagent
                         │
                       SLOT 7
                     FINAL CLI                ├── Main agent
                         │
                       SLOT 8
             INDEPENDENT VERIFIER              ├── Fresh context subagent
```

---

### SLOT 0 — AUTOMATED SELF-BOOTSTRAP SANDBOX

**Execution Steps**:
1. Check if `D:\.ltn-graph\state.json` exists. If `SLOT_0` is `COMPLETED` and container is running (`docker ps --filter name=ltn-sandbox`), advance immediately to `SLOT_1`.
2. Check if Docker engine is responsive (`docker ps`).
   - If Docker engine fails to connect: Launch Docker Desktop automatically via `run_command`:
     `Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"`
   - Poll `docker ps` until Docker daemon connects (wait up to 30s).
3. Run container initializer:
   `run_command`: `powershell -File D:\ltn-eng-workspace\sandbox-init.ps1`
4. Verify sandbox environment inside container:
   `run_command`: `docker exec ltn-sandbox rustc --version`
5. Record state in `D:\.ltn-graph\state.json`: set `SLOT_0` to `COMPLETED`, `active_slot` to `SLOT_1`.

---

### SLOT 1 — PREFLIGHT, AUDIT & CONTRACT FREEZE

1. Check `state.json`. If `SLOT_1` is `COMPLETED`, skip to `SLOT_2`.
2. Inspect Beam git state (`git -C D:\beam log -1 --format="%H %ai"`).
3. Initialize workspace `Cargo.toml` at `D:\ltn-eng-workspace\Cargo.toml`.
4. Freeze cross-layer schemas in `D:\.ltn-graph\contracts\`:
   - `project_profile.schema.json`
   - `architecture_plan.schema.json`
   - `solver_constraints.schema.json`
   - `execution_receipt.schema.json`
   - `observation_telemetry.schema.json`
5. Mark `SLOT_1` as `COMPLETED` in `D:\.ltn-graph\state.json`.

---

### SLOT 2 — LTN.ENG DETERMINISTIC ENGINE (`crates/ltn-core`)
1. Implement Typed IR adapted from Beam `beam-ir` (REAL).
2. Adapt Z3 solver from Beam `beam-solver` (REAL). SMT encoding for compute/memory/GPU/VRAM, MUS extraction.
3. Cryptographic state integrity engine.
4. Transactional state machine with atomic rollback.
5. Topology discovery adapters (`map-mtl`).
6. Validate: `docker exec ltn-sandbox cargo test -p ltn-core`. Mark `SLOT_2` as `COMPLETED` in `state.json`.

---

### SLOT 3 — PROJECT INTELLIGENCE & ARCHITECT (`crates/ltn-architect`)
1. AST & Manifest parsers (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `pom.xml`, `Dockerfiles`).
2. Workload Classifier engine.
3. Architecture Planner producing 3 models: `CHEAPEST`, `BALANCED`, `HIGH-PERFORMANCE`.
4. Missing Artifact Generator.
5. Validate: `docker exec ltn-sandbox cargo test -p ltn-architect`. Mark `SLOT_3` as `COMPLETED` in `state.json`.

---

### SLOT 4 — AGENT RUNTIME & TYPED TOOLS (`crates/ltn-agent`)
1. Sandboxed Agent Harness (`READ`, `PLAN`, `VALIDATE`, `MUTATE`, `VERIFY`, `ROLLBACK`).
2. Typed Tool Registry.
3. Compiler diagnostics integration (`--message-format=json`).
4. Deterministic Gate: Mutation plans MUST pass `ltn-core` Z3 validation.
5. Validate: `docker exec ltn-sandbox cargo test -p ltn-agent`. Mark `SLOT_4` as `COMPLETED` in `state.json`.

---

### SLOT 5 — PROVIDERS (`crates/ltn-providers`)
1. Actuators for Docker, AWS CLI, GCP CLI, Azure CLI, Local OS, Bare-Metal.
2. Honest Authentication Detection (`BLOCKED_UNAUTHENTICATED` if unauthenticated).
3. Validate: `docker exec ltn-sandbox cargo check -p ltn-providers`. Mark `SLOT_5` as `COMPLETED` in `state.json`.

---

### SLOT 6 — OPERATIONS & CHAOS (`crates/ltn-ops`)
1. Empirical Observer: live socket probes, process tables, HTTP health checks.
2. Drift Detection & Self-Healing via `ltn-replan`.
3. Chronos Chaos Engine.
4. MCP Server.
5. Validate: `docker exec ltn-sandbox cargo check -p ltn-ops`. Mark `SLOT_6` as `COMPLETED` in `state.json`.

---

### SLOT 7 — FINAL CLI (`crates/ltn-cli`)
1. Implement `ltn` CLI with subcommands: `init`, `doctor`, `analyze`, `architect`, `deploy`, `status`, `explain`, `snapshot`, `rollback`.
2. Build binary: `docker exec ltn-sandbox cargo build -p ltn-cli`. Mark `SLOT_7` as `COMPLETED` in `state.json`.

---

### SLOT 8 — INDEPENDENT VERIFIER & DOGFOOD MATRIX
1. Run quality gates in container:
   - `docker exec ltn-sandbox cargo fmt --check --manifest-path /workspace/Cargo.toml`
   - `docker exec ltn-sandbox cargo clippy --manifest-path /workspace/Cargo.toml --workspace --all-targets -- -D warnings`
   - `docker exec ltn-sandbox cargo test --manifest-path /workspace/Cargo.toml --workspace`
2. Run end-to-end integration tests against 6 synthetic workloads in `D:\ltn-eng-workspace\tests\fixtures\`:
   - React/Vite Static, Node/Postgres API, Python/vLLM GPU, Go Worker, Rust WASM, Stateful Memory Store.
3. Emit `D:\.ltn-graph\FINAL_REPORT.md`. Mark `state.json` as `ALL_SLOTS_COMPLETED`.

---

## 7 — EXECUTION START

**BEGIN IMMEDIATELY UPON INVOCATION.**

1. Inspect `D:\.ltn-graph\state.json`.
2. Execute **Slot 0 (Automated Self-Bootstrap Sandbox)**.
3. Proceed through slots continuously, writing state updates after each step.
