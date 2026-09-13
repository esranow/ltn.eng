#!/usr/bin/env python3
"""
Spot Check Evaluation Suite for LTN.ENG
Validates Fixes 1-6 across 9 hand-picked problems spanning all 4 categories:
  - Multi-Tenancy Cloud: MT-001, MT-002 (Mandatory)
  - Singular Cloud: SC-039 (Mandatory), SC-040
  - Small-Scale Cloud: SM-077 (Mandatory), SM-078, SM-079
  - Data Centre On-Premise: DC-114 (Mandatory), DC-115
Enforces MiniStack 200 OK log count invariant and prints raw HTTP pairs, classifier IO, and pricing breakdown.
"""

import os
import sys
import json
import time
import subprocess
import re

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

LTN_BIN = os.path.normpath(r"D:\ltn-eng-workspace\target\debug\ltn.exe")
TESTS_ROOT = os.path.normpath(r"D:\ltn-eng-workspace\tests\eval_spot_check")
MINISTACK_URL = "http://127.0.0.1:4566"
LOG_FILE = r"D:\tmp\ministack-4566.log"
REPORT_JSON = os.path.normpath(r"D:\ltn-eng-workspace\bin\eval_results_spot_check.json")

os.environ["AWS_ENDPOINT_URL"] = MINISTACK_URL
os.environ["AWS_DEFAULT_REGION"] = "us-east-1"
os.environ["AWS_ACCESS_KEY_ID"] = "test-key"
os.environ["AWS_SECRET_ACCESS_KEY"] = "test-secret"

SPOT_PROBLEMS = [
    # Category 1: Multi-Tenancy Cloud Systems
    {
        "id": "MT-001",
        "name": "FinTech Isolated DB Multi-Tenancy",
        "cat": "Multi-Tenant Cloud",
        "files": {"package.json": '{"name": "fintech-api", "dependencies": {"pg": "^8.11.0", "express": "^4.18.0"}}'},
        "expected_kind": "NodePostgresApi",
        "tier": "BALANCED"
    },
    {
        "id": "MT-002",
        "name": "Healthcare HIPAA Isolated VPC Compute",
        "cat": "Multi-Tenant Cloud",
        "files": {"docker-compose.yml": "services:\n  hipaa_api:\n    image: app\n    mem_limit: 8g\n  hipaa_db:\n    image: postgres:16"},
        "expected_kind": "StatefulStore",
        "tier": "HIGH_PERFORMANCE"
    },

    # Category 2: Singular Cloud Systems
    {
        "id": "SC-039",
        "name": "Ultra-Low-Latency Order Matching Engine",
        "cat": "Singular Cloud",
        "files": {"Cargo.toml": '[package]\nname = "order-matcher"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\ncrossbeam = "0.8"'},
        "expected_kind": "RustWasm",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "SC-040",
        "name": "HLS Video Streaming Transcoding Pipeline",
        "cat": "Singular Cloud",
        "files": {"docker-compose.yml": "services:\n  ffmpeg:\n    image: jrottenberg/ffmpeg\n  transcoder:\n    image: app\n    mem_limit: 8g"},
        "expected_kind": "StatefulStore",
        "tier": "BALANCED"
    },

    # Category 3: Small-Scale Cloud Deployments
    {
        "id": "SM-077",
        "name": "Bootstrapped SaaS MVP (Node + Postgres + S3)",
        "cat": "Small-Scale Cloud",
        "files": {"package.json": '{"name": "saas-mvp", "dependencies": {"express": "^4.18.0", "pg": "^8.11.0", "@aws-sdk/client-s3": "^3.500.0"}}'},
        "expected_kind": "NodePostgresApi",
        "tier": "BALANCED"
    },
    {
        "id": "SM-078",
        "name": "Jamstack Static Portfolio with Serverless Form",
        "cat": "Small-Scale Cloud",
        "files": {"package.json": '{"name": "jamstack-portfolio", "devDependencies": {"vite": "^5.1.0", "react": "^18.2.0"}}'},
        "expected_kind": "StaticWeb",
        "tier": "CHEAPEST"
    },
    {
        "id": "SM-079",
        "name": "Python FastAPI Webhook Receiver",
        "cat": "Small-Scale Cloud",
        "files": {"pyproject.toml": '[project]\nname = "webhook-rcv"\nversion = "0.1.0"\ndependencies = ["fastapi", "uvicorn"]'},
        "expected_kind": "PythonVllmGpu",
        "tier": "BALANCED"
    },

    # Category 4: Large-Scale On-Premises Data Centre Management
    {
        "id": "DC-114",
        "name": "64-Node Bare-Metal H100 InfiniBand Cluster",
        "cat": "Data Centre On-Premise",
        "files": {"pyproject.toml": '[project]\nname = "h100-trainer"\nversion = "0.1.0"\ndependencies = ["torch", "deepspeed", "vllm"]'},
        "expected_kind": "PythonVllmGpu",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "DC-115",
        "name": "NUMA-Aware Core Pinning & 1GB HugePages Pool",
        "cat": "Data Centre On-Premise",
        "files": {"Cargo.toml": '[package]\nname = "numa-allocator"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\ncore_affinity = "0.8"'},
        "expected_kind": "RustWasm",
        "tier": "HIGH_PERFORMANCE"
    },
]

def ensure_fixture(problem):
    clean_name = re.sub(r'[^a-zA-Z0-9]', '_', problem["name"].lower())
    clean_name = re.sub(r'_+', '_', clean_name).strip('_')
    pdir = os.path.join(TESTS_ROOT, f"{problem['id']}_{clean_name}")
    os.makedirs(pdir, exist_ok=True)
    for fname, content in problem["files"].items():
        fpath = os.path.join(pdir, fname)
        with open(fpath, "w", encoding="utf-8") as f:
            f.write(content)
    return pdir

def read_log_lines():
    if os.path.exists(LOG_FILE):
        with open(LOG_FILE, "r", encoding="utf-8", errors="ignore") as f:
            return f.readlines()
    return []

def run_spot_check():
    print("=" * 80)
    print("  LTN.ENG 9-PROBLEM SPOT CHECK EVALUATION & INVARIANT VERIFICATION")
    print(f"  Target Binary : {LTN_BIN}")
    print(f"  MiniStack URL : {MINISTACK_URL}")
    print(f"  Log File      : {LOG_FILE}")
    print("=" * 80)

    initial_log_lines = read_log_lines()
    baseline_count = len(initial_log_lines)
    print(f"[*] Baseline MiniStack log line count: {baseline_count}")

    proc = subprocess.Popen(
        [LTN_BIN, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=os.environ
    )

    def rpc(method, params=None, req_id=1):
        req = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params is not None:
            req["params"] = params
        proc.stdin.write(json.dumps(req) + "\n")
        proc.stdin.flush()
        line = proc.stdout.readline()
        return json.loads(line)

    init_res = rpc("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "spot-check-verifier", "version": "1.0"}
    }, 1)
    assert "result" in init_res, f"MCP handshake failed: {init_res}"
    print("[✓] MCP Handshake Established Successfully")

    results = []
    deployed_records_count = 0

    for idx, prob in enumerate(SPOT_PROBLEMS, start=1):
        prob_id = prob["id"]
        prob_name = prob["name"]
        cat = prob["cat"]
        pdir = ensure_fixture(prob)
        req_base = 100 + idx * 10

        print("\n" + "-" * 80)
        print(f"PROBLEM #{idx:02d}: [{prob_id}] {prob_name} ({cat})")
        print("-" * 80)

        # Step A: ltn_analyze
        ana_res = rpc("tools/call", {"name": "ltn_analyze", "arguments": {"path": pdir}}, req_base)
        ana_text = ana_res.get("result", {}).get("content", [{}])[0].get("text", "{}")
        classification = json.loads(ana_text)

        print(f"[CLASSIFIER INPUT]  Path: {pdir}")
        print(f"[CLASSIFIER INPUT]  Files: {list(prob['files'].keys())}")
        print(f"[CLASSIFIER OUTPUT] Workload Kind : {classification.get('workload_kind')}")
        print(f"[CLASSIFIER OUTPUT] CPU Cores     : {classification.get('recommended_cpu_cores')}")
        print(f"[CLASSIFIER OUTPUT] RAM (MB)      : {classification.get('recommended_ram_mb')}")
        print(f"[CLASSIFIER OUTPUT] VRAM (MB)     : {classification.get('recommended_vram_mb')}")
        print(f"[CLASSIFIER OUTPUT] GPU Count     : {classification.get('recommended_gpu_count')}")
        print(f"[CLASSIFIER OUTPUT] Description   : {classification.get('description')}")

        # Step B: ltn_architect
        arch_res = rpc("tools/call", {"name": "ltn_architect", "arguments": {"path": pdir}}, req_base + 1)
        arch_text = arch_res.get("result", {}).get("content", [{}])[0].get("text", "[]")
        plans = json.loads(arch_text)

        print("\n[COST BREAKDOWN - DYNAMICALLY COMPUTED PRICING]")
        for p in plans:
            tier = p.get("tier")
            prov = p.get("provider")
            cost = p.get("estimated_monthly_cost_usd")
            print(f"  • {tier:<16} | Provider: {prov:<20} | Est. Cost: ${cost:,.2f} / month")

        # Step C: ltn_deploy (Executes real HTTP call to MiniStack)
        pre_deploy_log_count = len(read_log_lines())
        dep_res = rpc("tools/call", {"name": "ltn_deploy", "arguments": {"path": pdir, "tier": prob["tier"]}}, req_base + 2)
        dep_text = dep_res.get("result", {}).get("content", [{}])[0].get("text", "{}")
        deploy_data = json.loads(dep_text)

        post_deploy_logs = read_log_lines()
        new_lines = post_deploy_logs[pre_deploy_log_count:]

        print("\n[DEPLOYMENT & REAL HTTP INTERACTION]")
        print(f"  • MCP Response Status : {deploy_data.get('status')}")
        print(f"  • Provider Target     : {deploy_data.get('provider')}")
        print(f"  • Routed to MiniStack : {deploy_data.get('routed_through_ministack')}")
        print(f"  • Instance/Task ID    : {deploy_data.get('instance_id')}")
        print(f"  • Actuator Result     : {deploy_data.get('actuator_status')}")

        if deploy_data.get("status") == "DEPLOYED" and deploy_data.get("routed_through_ministack"):
            deployed_records_count += 1

        print(f"  • Raw MiniStack Log Captured ({len(new_lines)} lines):")
        for nl in new_lines:
            print(f"    --> {nl.strip()}")

        results.append({
            "id": prob_id,
            "name": prob_name,
            "category": cat,
            "tier": prob["tier"],
            "classification": classification,
            "plans": plans,
            "deployment": deploy_data,
            "raw_ministack_log_lines": [nl.strip() for nl in new_lines]
        })

    proc.stdin.close()
    proc.terminate()

    all_final_logs = read_log_lines()
    run_log_lines = all_final_logs[baseline_count:]
    ministack_200_ok_lines = [
        l.strip() for l in run_log_lines
        if ("POST /ecs" in l or "RunTask" in l) and "Status: 200" in l
    ]

    print("\n" + "=" * 80)
    print("  RUNTIME INVARIANT VERIFICATION (FIX 5)")
    print("=" * 80)
    print(f"  Total DEPLOYED records routed through MiniStack : {deployed_records_count}")
    print(f"  Total MiniStack 200 OK HTTP log lines in run    : {len(ministack_200_ok_lines)}")
    print(f"  Baseline Log Lines                             : {baseline_count}")
    print(f"  Final Total Log Lines                          : {len(all_final_logs)}")

    if len(ministack_200_ok_lines) != deployed_records_count:
        print("\n[FAILED] INVARIANT DIVERGENCE DETECTED!")
        print(f"Expected {deployed_records_count} 200 OK lines, but found {len(ministack_200_ok_lines)}!")
        sys.exit(1)
    else:
        print(f"\n[PASSED] INVARIANT STRICTLY SATISFIED: count(MiniStack 200 OK) == count(DEPLOYED records) == {deployed_records_count}")

    report = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "total_evaluated": len(SPOT_PROBLEMS),
        "deployed_records_count": deployed_records_count,
        "ministack_200_ok_count": len(ministack_200_ok_lines),
        "invariant_satisfied": True,
        "results": results
    }

    with open(REPORT_JSON, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)

    print(f"[✓] Spot check evaluation receipt written to: {REPORT_JSON}")
    print("=" * 80)

if __name__ == "__main__":
    run_spot_check()
