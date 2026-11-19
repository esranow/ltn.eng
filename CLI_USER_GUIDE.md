# LTN Manager — Command Line & Interactive Terminal User Guide

Welcome to **LTN Manager**. This guide explains how to operate the software suite through clean, user-friendly terminal commands and the interactive Terminal Control Panel.

---

## ⚡ 1. How to Launch

`ltn` is registered globally in your environment PATH (`D:\ltn-eng-workspace\bin`). You can run `ltn` from any terminal session (PowerShell, Command Prompt, or WSL):

### Interactive Terminal App Mode
Simply type `ltn` to open the interactive Control Panel:
```bash
ltn
```

```text
================================================================
  ⚡ LTN Manager — Interactive System Control Panel
================================================================

Select an action:
  [1] 🩺 Run System Diagnostics
  [2] 🔍 Analyze Project Workload
  [3] 🏛️ View Architecture Options
  [4] ⚡ Deploy Project
  [5] 📈 Check System Health & Status
  [6] 🧠 View Architecture Analysis
  [7] 📸 Manage Snapshots & Restores
  [8] ⚙️ System Settings
  [0] 🚪 Exit Control Panel
```

---

## 📖 2. Command Reference

### `ltn doctor`
Checks execution readiness, system sandbox, and cloud provider readiness.
```bash
ltn doctor
```
**Output Example:**
```text
🩺 System Health & Provider Check:
  [✓] Execution Sandbox: Operational
  [✓] Constraint Verification Engine: Ready
  [✓] Local System Provider: Ready
  [✓] Local Docker: Ready
  [!] Amazon Web Services: Credentials Required
  [!] Google Cloud Platform: Credentials Required
  [!] Microsoft Azure: Credentials Required
```

---

### `ltn analyze <path>`
Scans project files, identifies the application type, and calculates recommended hardware resources.
```bash
ltn analyze ./my-project
```
**Output Example:**
```text
📊 Workload Analysis for './my-project':
  Application Type: Python High-Performance LLM/GPU Inference Workload
  Detected Files:   ["pyproject.toml/requirements.txt"]
  Recommended CPU:  8 cores
  Recommended Memory: 32768 MB
  Recommended GPU VRAM: 24576 MB
```

---

### `ltn architect <path>`
Generates 3 tailored architectural options with estimated costs:
- **Cost-Effective**: Local container setup ($0/mo).
- **Balanced Production**: High-availability cloud setup ($35/mo).
- **High-Performance Cluster**: GPU-accelerated cluster setup ($280/mo).

```bash
ltn architect ./my-project
```

---

### `ltn deploy <path> [--tier BALANCED]`
Deploys your application through automated step-by-step validation and verification.
```bash
ltn deploy ./my-project --tier BALANCED
```
**Output Example:**
```text
⚡ Deploying './my-project' [BALANCED]...
  [✓] Step 1/5: Scanning project manifests and structure
  [✓] Step 2/5: Generating architectural blueprint
  [✓] Step 3/5: Verifying hardware resource availability
  [✓] Step 4/5: Applying configuration and starting containers
  [✓] Step 5/5: Running health probes and verifying endpoint status
🎉 Deployment completed successfully!
```

---

### `ltn status`
Checks active network connections, running service count, and endpoint response status.
```bash
ltn status
```

---

### `ltn snapshot` & `ltn rollback`
- `ltn snapshot`: Saves a restore point snapshot.
- `ltn rollback`: Restores system to the previous snapshot.
```bash
ltn snapshot
ltn rollback
```

---

### `ltn config`
Views or updates default cloud provider settings.
```bash
ltn config
ltn config --set-provider AWS
```

---

### `ltn run <manifest>`
Runs containers deterministically from any manifest without requiring Docker Desktop or Kubernetes daemons.
- **Autodetects Manifests**: Docker Compose (`docker-compose.yml`), Kubernetes (`Deployment`, `Pod`), K6s, or custom YAML.
- **Z3 SMT Mathematical Verification**: Proves CPU, RAM, and port constraints before execution, preventing autoscaler thrashing and runtime resource contention.
- **Pure-Rust Container Isolation**: Launches workloads under hard CPU/RAM ceilings and tracks states with cryptographic SHA-256 integrity hashes.

```bash
# Run from Docker Compose
ltn run ./docker-compose.yml

# Run from Kubernetes manifest
ltn run ./kubernetes.yaml

# Run from K6s load spec
ltn run ./k6s.yaml
```

**Output Example:**
```text
⚡ Launching workload deterministically from 'docker-compose.yml'...
  [✓] Detected manifest format: DockerCompose
  [✓] Step 1/3: Ingesting manifest and normalizing to typed IR (2 workloads)
  [✓] Step 2/3: Proving hardware constraint feasibility via Z3 SMT Prover
  [✓] Step 3/3: Spawning sandboxed container runtimes with strict CPU/RAM bounds
  [✓] Container 'api' (doc-e4b2a19c) active on port 3000 [CPU: 2 cores, RAM: 1024MB]
  [✓] Container 'db' (doc-90f711cb) active on port 5432 [CPU: 4 cores, RAM: 4096MB]
🔒 Cumulative State Integrity Hash: 4a2f8b91e03d4c5a6f7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f
🎉 Deterministic workload running stably with zero daemon overhead!
```
