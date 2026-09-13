# LTN.ENG — Deep Architectural Specification

## 1. Core Architectural Guarantees & Invariants

LTN.ENG is engineered around five fundamental invariants:

1. **Workload Diversity Invariant**: The engine never assumes all projects are Docker web apps. Web frontends, REST APIs, GPU LLM inference nodes, background worker pools, WebAssembly micro-modules, and stateful databases require fundamentally distinct architectural reasoning.
2. **Deterministic Constraint Gate**: No infrastructure modification or actuator payload is executed without passing Z3 SMT solver verification.
3. **Empirical Observer Ground Truth**: Operational health is declared exclusively through live socket probes, process table queries, and HTTP health probes.
4. **Honest Authentication Invariant**: Unauthenticated provider calls fail explicitly with `BLOCKED_UNAUTHENTICATED` state receipts rather than masking failures.
5. **Transactional Atomic Rollback**: All state transitions record SHA-256 state hashes. Failure at any point in the agent harness lifecycle triggers immediate atomic rollback to the prior snapshot.

---

## 2. Layer Deep Dives

### 2.1 Agentic Intelligence Layer (`ltn-architect` & `ltn-agent`)
- **Manifest Parser**: Discovers static manifests in project root:
  - `package.json` → Node.js / React / Vite
  - `Cargo.toml` → Rust / WASM
  - `pyproject.toml` / `requirements.txt` → Python / PyTorch / vLLM
  - `go.mod` → Go Distributed Worker
  - `pom.xml` → Java Service
  - `Dockerfile` / `docker-compose.yml` → Container specs
- **Workload Classifier**: Evaluates manifests and project directory names against heuristic rules, outputting target hardware requirements (CPU cores, RAM MB, VRAM MB, GPU count).
- **3-Tier Architecture Planner**: Produces 3 complete deployment plan specifications:
  - `CHEAPEST`: Cost-optimized local single-container setup ($0/mo).
  - `BALANCED`: Production 2-tier high-availability setup ($35/mo).
  - `HIGH-PERFORMANCE`: Distributed GPU-accelerated cluster setup ($280/mo).

### 2.2 Deterministic SMT Core (`ltn-core`)
- **Typed IR**: Formal data structures representing hardware resource blocks and workload constraints (`CpuCores`, `RamMb`, `VramMb`, `GpuCount`, `NetworkGbps`, `DataLocality`, `StrictIsolation`).
- **Z3 SMT Solver & MUS Extractor**: Encodes resource allocations as linear SMT formulas. If solver yields `UNSAT`, extracts the **Minimal Unsatisfiable Core (MUS)** detailing exact hardware requirement conflicts.
- **SHA-256 State Integrity Hasher**: Generates deterministic hex strings for all JSON artifacts and execution manifests.
- **Transactional State Machine**: Maintains in-memory and disk snapshot histories with full rollback functionality.

### 2.3 Actuators & Providers (`ltn-providers`)
- Actuators wrap concrete execution logic:
  - `DockerActuator`: Container deployment & network mapping.
  - `AwsActuator`: Amazon ECS task definitions & RDS provisioning.
  - `GcpActuator`: Google GKE cluster workload submission.
  - `AzureActuator`: Azure AKS cluster provisioning.
  - `LocalOsActuator`: Process spawning and native binary execution.
  - `BareMetalActuator`: SSH/systemd bare-metal node orchestration.

### 2.4 Empirical Operations (`ltn-ops`)
- **Empirical Observer**: Performs real-time TCP socket connection probes, process table lookups, and HTTP status code verification.
- **Self-Healing Replan Engine (`ltn-replan`)**: Evaluates observer telemetry against desired state, triggering automated corrective re-planning when drift occurs.
- **Chronos Chaos Engine**: Simulates network packet loss, process crashes, and memory pressure to verify self-healing resilience.
- **MCP Server**: Implements Model Context Protocol endpoints for seamless LLM sidecar integration.
