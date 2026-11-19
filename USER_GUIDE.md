# LTN.ENG — Complete User & Operator Guide

Welcome to **LTN.ENG**, an autonomous, deterministic cloud and edge workload orchestration engine powered by Z3 SMT constraint solving and empirical OS observer telemetry.

---

## Key Concepts & Architecture

1. **Deterministic SMT Core (`ltn-core`)**:
   Formal hardware bound checking (CPU, RAM, VRAM, GPU, Network) using Z3 SMT solver and Minimal Unsatisfiable Core (MUS) extraction.

2. **Project Intelligence (`ltn-architect`)**:
   Automatic manifest discovery (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `pom.xml`, `Dockerfile`) and 3-tier architecture planner (`CHEAPEST`, `BALANCED`, `HIGH-PERFORMANCE`).

3. **Sandboxed Agent Harness (`ltn-agent`)**:
   Deterministic 6-phase lifecycle: `READ` → `PLAN` → `VALIDATE` → `MUTATE` → `VERIFY` → `ROLLBACK`.

4. **Multi-Provider Actuators (`ltn-providers`)**:
   Actuators for Docker, AWS CLI, GCP CLI, Azure CLI, Local OS, and Bare-Metal with honest authentication checking (`BLOCKED_UNAUTHENTICATED`).

5. **Empirical Operations & Chaos (`ltn-ops`)**:
   Live socket probes, HTTP health monitoring, self-healing replanning (`ltn-replan`), Chronos Chaos Engine, and Model Context Protocol (MCP) server integration.

---

## Global CLI Installation

`ltn` is installed globally on your machine:
- Executable directory: `D:\ltn-eng-workspace\bin`
- Added to User `PATH`.

Verify installation in PowerShell or Command Prompt:
```bash
ltn doctor
```

---

## Subcommand Reference

| Command | Usage | Description |
| ------- | ----- | ----------- |
| `ltn init` | `ltn init` | Initializes graph directories and configuration files. |
| `ltn doctor` | `ltn doctor` | Verifies container sandbox, Z3 engine, and provider credentials. |
| `ltn analyze` | `ltn analyze <path>` | Discovers manifests and classifies project workload type. |
| `ltn architect` | `ltn architect <path>` | Generates 3-tier architecture models with cost estimations. |
| `ltn deploy` | `ltn deploy <path> [--tier BALANCED]` | Validates via Z3 SMT solver and deploys workload. |
| `ltn status` | `ltn status` | Queries socket probes, process status, and HTTP telemetry. |
| `ltn explain` | `ltn explain` | Displays formal mathematical proof and MUS core reasoning. |
| `ltn snapshot` | `ltn snapshot` | Creates an immutable SHA-256 state snapshot. |
| `ltn rollback` | `ltn rollback` | Atomically rolls back to the previous state snapshot. |
| `ltn config` | `ltn config [--set-provider AWS]` | Manages configuration settings in `D:\.ltn-graph\config.toml`. |

---

## Quickstart Examples

### 1. Classify a Project Workload
```bash
ltn analyze ./my-app
```

### 2. Generate 3 Architecture Plans
```bash
ltn architect ./my-app --output plan.json
```

### 3. Deploy to Production
```bash
ltn deploy ./my-app --tier HIGH_PERFORMANCE --output receipt.json
```

### 4. Monitor Health
```bash
ltn status
```

### 5. Launch Deterministic Containers Directly
Execute any Docker Compose, Kubernetes, K6s, or custom YAML manifest deterministically without Docker or K8s daemons:
```bash
ltn run ./my-app/docker-compose.yml
# or
ltn run ./my-app/kubernetes.yaml
```

---

## Configuration File Schema (`D:\.ltn-graph\config.toml`)

```toml
[provider]
default_provider = "LOCAL_DOCKER"
default_tier = "BALANCED"
aws_region = "us-east-1"

[telemetry]
probe_interval_sec = 10
auto_heal_enabled = true
max_fix_attempts = 3
```

---

## Troubleshooting & Verification

- **State File**: `D:\.ltn-graph\state.json`
- **Contracts**: `D:\.ltn-graph\contracts\*.schema.json`
- **Final Report**: `D:\.ltn-graph\FINAL_REPORT.md`

