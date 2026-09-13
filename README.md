# LTN.ENG — Master Workload & Cloud Orchestration Engine

[![Build Status](https://img.shields.io/badge/Build-Passing-brightgreen.svg)]()
[![Tests](https://img.shields.io/badge/Workspace%20Tests-22%2F22%20Passed-blue.svg)]()
[![Clippy](https://img.shields.io/badge/Clippy-0%20Warnings-success.svg)]()
[![Rust Toolchain](https://img.shields.io/badge/Rust-1.98.0-orange.svg)]()
[![Prover](https://img.shields.io/badge/Z3%20SMT-4.13.3-purple.svg)]()

**LTN.ENG** is an autonomous, deterministic cloud and edge workload orchestration engine powered by **Z3 SMT constraint solving**, **SHA-256 state integrity hashing**, and **empirical OS/network observer telemetry**.

Unlike conventional DevOps toolchains that rely on non-deterministic heuristic prompts, LTN.ENG enforces **formal mathematical proofs** for all resource allocations and topology bounds before mutating any target infrastructure.

---

## 🏛️ System Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  AGENTIC INTELLIGENCE LAYER (ltn-architect & ltn-agent)                    │
│  - Manifest Parsers (package.json, Cargo.toml, pyproject.toml, go.mod)    │
│  - Workload Classifier (StaticWeb, NodePostgresApi, PythonVllmGpu, etc.)   │
│  - 3-Tier Architecture Planner (CHEAPEST, BALANCED, HIGH-PERFORMANCE)      │
│  - Sandboxed Agent Harness (READ → PLAN → VALIDATE → MUTATE → VERIFY)       │
└────────────────────────────────────┬────────────────────────────────────────┘
                                     │ Intent & Resource Demands
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│  LTN.ENG DETERMINISTIC ENGINE (ltn-core)                                    │
│  - Typed IR & Hardware Block Specifications                                 │
│  - Z3 SMT Constraint Prover & Minimal Unsatisfiable Core (MUS) Extraction   │
│  - SHA-256 State Integrity Hasher                                           │
│  - Transactional State Machine with Atomic Rollback                         │
└────────────────────────────────────┬────────────────────────────────────────┘
                                     │ Validated ExecManifest
                                     ▼
┌────────────────────────────────────┴────────────────────────────────────────┐
│  ACTUATOR & PROVIDER LAYER (ltn-providers)                                  │
│  - Local Docker Actuator                                                    │
│  - Cloud Actuators: AWS (ECS/RDS), GCP (GKE), Azure (AKS)                   │
│  - Local OS & Bare-Metal Actuators                                          │
│  - Honest Authentication Checker (BLOCKED_UNAUTHENTICATED)                 │
└────────────────────────────────────┬────────────────────────────────────────┘
                                     │ Deployed Infrastructure
                                     ▼
┌────────────────────────────────────┴────────────────────────────────────────┐
│  EMPIRICAL OBSERVER LAYER (ltn-ops)                                         │
│  - Live Socket Probes & Latency Metrics                                     │
│  - Process Table Monitoring                                                 │
│  - HTTP Endpoint Health Verification                                        │
│  - Self-Healing Drift Re-planner (ltn-replan)                               │
│  - Chronos Chaos Fault Injection Engine                                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## ⚡ Quickstart

### 1. Verification & Diagnostics
Run `ltn doctor` to verify container sandbox, Z3 solver readiness, and provider authentication:
```bash
ltn doctor
```

### 2. Workload Analysis
Analyze any project codebase to classify workload requirements:
```bash
ltn analyze ./my-app
```

### 3. Architecture Model Generation
Synthesize 3-tier architectural options (`CHEAPEST`, `BALANCED`, `HIGH-PERFORMANCE`) with cost estimations:
```bash
ltn architect ./my-app
```

### 4. Deterministic Deployment
Deploy with formal Z3 SMT validation and agent harness verification:
```bash
ltn deploy ./my-app --tier BALANCED
```

### 5. Interactive Terminal App
Launch the interactive Terminal UI App loop:
```bash
ltn
```

---

## 📦 Workspace Crate Layout

- [`crates/ltn-core`](file:///D:/ltn-eng-workspace/crates/ltn-core): Typed IR, Z3 SMT & MUS solver, SHA-256 hasher, state machine, settings manager.
- [`crates/ltn-architect`](file:///D:/ltn-eng-workspace/crates/ltn-architect): AST & manifest discovery, workload classifier, 3-model architecture planner.
- [`crates/ltn-agent`](file:///D:/ltn-eng-workspace/crates/ltn-agent): Sandboxed agent harness, tool registry, cargo diagnostics parser, deterministic gate.
- [`crates/ltn-providers`](file:///D:/ltn-eng-workspace/crates/ltn-providers): Docker, AWS, GCP, Azure, Local OS, and Bare Metal actuators with honest authentication checking.
- [`crates/ltn-ops`](file:///D:/ltn-eng-workspace/crates/ltn-ops): Empirical telemetry probes, self-healing replanner, Chronos chaos engine, MCP server.
- [`crates/ltn-cli`](file:///D:/ltn-eng-workspace/crates/ltn-cli): Production CLI binary (`ltn`) and interactive terminal app menu.

---

## 📚 Complete Documentation Index

- 📖 [User & Operator Guide](USER_GUIDE.md)
- 🖥️ [CLI Subcommand Reference](CLI_USER_GUIDE.md)
- 🏗️ [Architectural Specification](DOCS/ARCHITECTURE.md)
- 🔌 [Crate API Reference](DOCS/API_REFERENCE.md)
- 📜 [Cross-Layer JSON Schemas & Contracts](DOCS/CONTRACTS.md)
- 🔧 [Troubleshooting & Diagnostics Guide](DOCS/TROUBLESHOOTING.md)
