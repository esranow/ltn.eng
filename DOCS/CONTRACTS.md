# LTN.ENG — Cross-Layer JSON Schemas & Contracts

All cross-layer data exchange in LTN.ENG is strictly typed and validated against JSON Schemas frozen in `D:\.ltn-graph\contracts\`.

---

## 1. Project Profile Contract (`project_profile.schema.json`)
Describes project manifests, workload classification, dependency graph, and hardware demands.
```json
{
  "project_id": "proj-12345",
  "project_path": "/workspace/tests/fixtures/python_vllm_gpu",
  "workload_type": "PYTHON_VLLM_GPU",
  "detected_manifests": ["pyproject.toml/requirements.txt"],
  "dependencies": ["vllm", "torch"],
  "resource_demands": {
    "cpu_cores": 8,
    "memory_mb": 32768,
    "gpu_count": 1,
    "vram_mb": 24576,
    "storage_gb": 100
  }
}
```

---

## 2. Architecture Plan Contract (`architecture_plan.schema.json`)
Represents synthesized 3-tier architectural options (`CHEAPEST`, `BALANCED`, `HIGH_PERFORMANCE`).
```json
{
  "plan_id": "plan-9876",
  "tier": "BALANCED",
  "provider": "AWS",
  "nodes": [
    {
      "name": "app-node",
      "role": "app-server",
      "image": "app:latest",
      "ports": ["80:8080"],
      "environment": {}
    }
  ],
  "missing_artifacts": [],
  "estimated_monthly_cost_usd": 35.0
}
```

---

## 3. Solver Constraints Contract (`solver_constraints.schema.json`)
Constraints supplied to the Z3 SMT Prover.
```json
{
  "solver_id": "smt-001",
  "max_cpu_cores": 16,
  "max_memory_mb": 65536,
  "max_gpu_count": 2,
  "max_vram_mb": 49152,
  "network_latency_ms_max": 20,
  "hard_constraints": ["CpuCores(8)", "VramMb(24576)"]
}
```

---

## 4. Execution Receipt Contract (`execution_receipt.schema.json`)
Emitted after completed deployment with cryptographic state hashes.
```json
{
  "receipt_id": "receipt-5544",
  "timestamp": "2026-09-05T14:50:00Z",
  "status": "SUCCESS",
  "prev_state_hash": "a1b2c3d4...",
  "new_state_hash": "e5f6g7h8...",
  "actions_executed": [
    "READ: scanned project files",
    "PLAN: generated BALANCED architecture plan",
    "VALIDATE: Z3 SMT constraint satisfied",
    "MUTATE: container deployed",
    "VERIFY: socket probe healthy"
  ],
  "endpoints": ["http://localhost:8080"]
}
```

---

## 5. Observation Telemetry Contract (`observation_telemetry.schema.json`)
Live telemetry collected by the Empirical Observer.
```json
{
  "telemetry_id": "telem-8899",
  "timestamp": "2026-09-05T14:50:05Z",
  "socket_probes": [
    { "address": "127.0.0.1:8080", "reachable": true, "latency_ms": 1.2 }
  ],
  "process_probes": [
    { "name": "ltn-sandbox", "pid": 100, "running": true }
  ],
  "http_probes": [
    { "url": "http://localhost:8080", "status_code": 200, "latency_ms": 4.5 }
  ],
  "drift_detected": false
}
```
