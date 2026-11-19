# LTN.ENG — Architectural Gap Analysis & Production Remediation Plan

**Audit Target**: `LTN.ENG` (`D:\ltn-eng-workspace`)  
**Status**: DRAFT FIX PLAN (Awaiting User Approval)  
**Objective**: Transform LTN.ENG from an in-memory architectural simulator into a real, production-grade infrastructure orchestration engine.

---

## 1. Executive Summary & Root Cause Analysis

The fundamental architectural flaw in the current implementation of LTN.ENG is **mock encapsulation**:
Instead of delegating low-level kernel and cloud operations to battle-tested engines (Docker, Kubernetes, OpenTofu, SSH), LTN.ENG attempted to mock them with lightweight Rust structs, in-memory HashMaps, and mock HTTP servers (`ministack-service.py`).

### The Architectural Pivot
LTN.ENG should not reinvent Docker, Linux cgroups, or cloud hypervisors. LTN.ENG’s true superpower is its **formal SMT constraint gate, deterministic state machine, and agentic workflow**.
To make it real, LTN.ENG must pivot from a **simulator** into an **actual controller** that drives real underlying engines.

```
┌────────────────────────────────────────────────────────┐
│                   LTN.ENG CONTROLLER                   │
│   (AST Classifier -> SMT Constraint Gate -> State Hash)│
└───────────────────────────┬────────────────────────────┘
                            │ Real Actuators
        ┌───────────────────┼───────────────────┐
        ▼                   ▼                   ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Docker Daemon │   │ OpenTofu CLI  │   │  russh (SSH)  │
│  (via bollard)│   │  (Subprocess) │   │ (Bare Metal)  │
└───────────────┘   └───────────────┘   └───────────────┘
```

---

## 2. Component Gap Breakdown & Concrete Fixes

### Gap 1: Container Runtime Execution (`ltn-container` & `ltn-providers`)
* **Current State**: `DeterministicContainerEngine` inserts an in-memory struct into a `HashMap` and drops it. `DockerActuator` generates random UUIDs (`doc-<uuid>`). No containers are spawned on the host OS.
* **Severity**: **BLOCKER**
* **Root Cause**: Missing container runtime client.
* **Concrete Fix**:
  1. Add `bollard = "0.16"` to `crates/ltn-container/Cargo.toml` (official Rust Docker API client).
  2. Implement `BollardContainerDriver` in `crates/ltn-container/src/runtime/`:
     - Connect to local Docker daemon (`//./pipe/docker_engine` on Windows or `/var/run/docker.sock` on Linux).
     - `pull_image(image_name)`: stream real Docker pull progress.
     - `create_and_start(workload)`: call `docker.create_container` with real `HostConfig` (setting CPU shares, memory limit bytes, port bindings, and volume mounts).
     - `stream_logs(container_id)`: stream stdout/stderr.
     - `inspect_stats(container_id)`: read real live CPU/RAM utilization.
  3. Update `ltn run <manifest>` to execute real containers and verify ports are listening.
* **Effort**: **Medium (M)** | ~400 LOC.
* **Test to Prove**: Run `ltn run docker-compose.yml`, execute `docker ps` on host, verify real container IDs, and `curl http://localhost:port`.

---

### Gap 2: Cloud Infrastructure & IaC Provisioning (`ltn-providers` & `ltn-core`)
* **Current State**: Actuators send unauthenticated HTTP requests to local port 4566 (`ministack-service.py`). `MapMtlInfraProvisioner` writes a static `.tf` string to disk and marks it `PROVISIONED_SUCCESS` without running Terraform.
* **Severity**: **BLOCKER**
* **Root Cause**: Stub cloud client without real cloud provider or IaC execution engine.
* **Concrete Fix**:
  1. Add an `IacExecutionEngine` in `crates/ltn-providers/src/actuators/tofu.rs`:
     - Bundle or detect `tofu` (OpenTofu) / `terraform` binary on PATH.
     - Write generated `.tf` files to `.ltn/infra/<target>/`.
     - Execute `tofu init -no-color`.
     - Execute `tofu plan -out=tfplan -no-color`.
     - Execute `tofu apply -auto-approve tfplan -json` and stream progress to CLI.
     - Store and parse the output JSON to extract real VPC IDs, instance IPs, and connection endpoints.
  2. Deprecate `ministack-service.py` entirely for production runs.
* **Effort**: **Large (L)** | ~600 LOC.
* **Test to Prove**: Run `ltn deploy --provider AWS`, verify real OpenTofu run creates real resources on test AWS account, and verify state file exists.

---

### Gap 3: Bare-Metal Fleet Management & Discovery (`ltn-providers/src/actuators/bare_metal.rs`)
* **Current State**: Pings port 22 with 200ms TCP timeout. If open, clones the local host machine's own CPU/RAM specs as the remote node's specs. Performs zero remote execution.
* **Severity**: **BLOCKER**
* **Root Cause**: Missing remote execution protocol.
* **Concrete Fix**:
  1. Add `russh = "0.45"` and `russh-keys = "0.45"` to `crates/ltn-providers/Cargo.toml`.
  2. Implement `SshRemoteExecutor`:
     - Load SSH private key from `~/.ssh/id_ed25519` or `~/.ssh/id_rsa`.
     - Connect to target node IP over SSH.
     - **Discovery**: Execute `uname -a`, `nproc`, `cat /proc/meminfo`, and `lspci | grep -i nvidia`. Parse real hardware specs.
     - **Provisioning**: SFTP systemd unit file to `/etc/systemd/system/ltn-<role>.service`, run `sudo systemctl daemon-reload && sudo systemctl enable --now ltn-<role>`.
     - **Health Verification**: Query `systemctl is-active ltn-<role>`.
* **Effort**: **Medium (M)** | ~500 LOC.
* **Test to Prove**: Spin up a real Linux VM (e.g., WSL2 or multipass instance), run `ltn deploy --bare-metal <vm-ip>`, verify real service running via `ssh <vm-ip> systemctl status`.

---

### Gap 4: True Dynamic Architecture Planning (`ltn-architect` & `ltn-agent`)
* **Current State**: Classifier matches directory strings (`contains("64_node")`, `contains("webhook")`). The planner emits 6 hardcoded static templates. The `--cto` flag returns a hardcoded mock string.
* **Severity**: **MAJOR**
* **Root Cause**: Hardcoded heuristics replacing actual AI model reasoning.
* **Concrete Fix**:
  1. Fix `crates/ltn-agent/src/router/universal.rs`:
     - Implement real HTTP POST client using `ureq` to live LLM endpoints (OpenRouter, DeepSeek, Gemini, Anthropic, or local Ollama).
     - Send the actual `cto_system_prompt()` with the discovered codebase AST.
  2. Require the LLM to output a strictly typed JSON Schema matching `ltn_core::ir::Architecture`:
     - Dynamically declare service names, container base images, ports, environment variables, compute requirements, and dependencies.
  3. Pipe that dynamically generated architecture through `SmtSolver::validate_architecture` before deployment.
* **Effort**: **Medium (M)** | ~450 LOC.
* **Test to Prove**: Run `ltn architect` on an arbitrary repository (e.g. Flask + Redis + Postgres), verify generated architecture dynamically includes all 3 services rather than assuming vLLM GPU inference.

---

### Gap 5: Production Observability & Active Self-Healing (`ltn-ops`)
* **Current State**: Pings a single URL via HTTP GET; stores 100 samples in an in-memory buffer. `replan.rs` and `daemon.rs` return hardcoded text strings saying containers were restarted without restarting anything.
* **Severity**: **MAJOR**
* **Root Cause**: Mocked reconciliation loop and lack of persistent monitoring.
* **Concrete Fix**:
  1. Connect `ReconciliationDaemon` to the new `BollardContainerDriver` or `SshRemoteExecutor`:
     - When health check fails: invoke `driver.restart_container(id)` or `ssh.restart_service(name)`.
     - If restart fails 3 times: trigger rollback via `StateMachine::rollback()`.
  2. Implement Alerting Webhook Dispatcher:
     - Allow configuring Slack/Discord/PagerDuty webhook URLs in `~/.ltn/config.toml`.
     - On drift detection or auto-heal event, POST structured alert payload.
* **Effort**: **Small (S)** | ~250 LOC.
* **Test to Prove**: Kill an active container manually, run `ltn reconcile`, verify the container is physically restarted by the daemon, and an alert is sent.

---

## 3. Prioritized Implementation Roadmap

| Step | Milestone | Crates Impacted | Dependencies | Outcome |
| :--- | :--- | :--- | :--- | :--- |
| **Phase 1** | **Real Container Runtime** | `ltn-container`, `ltn-providers` | Docker Engine / `bollard` | `ltn run` actually spins up, manages, and tracks real containers on the OS. |
| **Phase 2** | **Live LLM Router & Dynamic Architecture** | `ltn-agent`, `ltn-architect` | Live API Key / Ollama | `ltn architect --cto` dynamically designs architectures for any codebase. |
| **Phase 3** | **OpenTofu Real Cloud Actuator** | `ltn-providers`, `ltn-core` | OpenTofu binary on PATH | `ltn deploy --provider AWS` provisions real cloud infrastructure. |
| **Phase 4** | **Real SSH Bare-Metal Actuator** | `ltn-providers` | `russh` crate | Bare-metal discovery and provisioning works against real remote Linux nodes. |
| **Phase 5** | **Closed-Loop Reconciliation & Alerting** | `ltn-ops` | Phase 1 & Phase 3 | Auto-healing physically restarts degraded infrastructure and notifies webhooks. |

---

## 4. Next Step
Before writing any code or modifying the codebase, please review this roadmap and confirm your approval to proceed with **Phase 1 (Real Container Runtime with Bollard/Docker API)**.
