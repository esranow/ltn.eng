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
