#!/usr/bin/env python3
"""
Audited Spot-Check Verification for LTN.ENG
Enforces Strict Runtime Invariant:
  count(MiniStack log lines with 'Status: 200') == count(eval records with status == 'DEPLOYED' that routed through MiniStack)
Tests 8 hand-picked problems across all 4 categories:
  - MT-002 (Multi-Tenant Cloud)
  - MT-004 (Multi-Tenant Cloud)
  - SC-039 (Singular Cloud)
  - SC-041 (Singular Cloud)
  - SM-077 (Small-Scale Cloud)
  - SM-080 (Small-Scale Cloud)
  - DC-114 (Data Centre On-Premise)
  - DC-116 (Data Centre On-Premise)
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
TESTS_ROOT = os.path.normpath(r"D:\ltn-eng-workspace\tests\eval_150_system_design")
MINISTACK_LOG = os.path.normpath(r"D:\tmp\ministack-4566.log")
MINISTACK_URL = "http://127.0.0.1:4566"
OUT_JSON = os.path.normpath(r"D:\ltn-eng-workspace\bin\audit_spot_check_results.json")

os.environ["AWS_ENDPOINT_URL"] = MINISTACK_URL
os.environ["AWS_DEFAULT_REGION"] = "us-east-1"
os.environ["AZURE_ENDPOINT_URL"] = MINISTACK_URL
os.environ["GCP_ENDPOINT_URL"] = MINISTACK_URL

SPOT_PROBLEMS = [
    {
        "id": "MT-002",
        "name": "Healthcare HIPAA Isolated VPC Compute",
        "cat": "Multi-Tenant Cloud",
        "dir_pattern": "MT-002_healthcare_hipaa_isolated_vpc_compute_stateful_store_db",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "MT-004",
        "name": "Multi-Tenant KMS Envelope Encryption",
        "cat": "Multi-Tenant Cloud",
        "dir_pattern": "MT-004_multi_tenant_kms_envelope_encryption_rust_wasm",
        "tier": "BALANCED"
    },
    {
        "id": "SC-039",
        "name": "Ultra-Low-Latency Order Matching Engine",
        "cat": "Singular Cloud",
        "dir_pattern": "SC-039_ultra_low_latency_order_matching_engine_rust_wasm",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "SC-041",
        "name": "1M Events/Sec IoT Stream Ingestion Mesh",
        "cat": "Singular Cloud",
        "dir_pattern": "SC-041_1m_events_sec_iot_stream_ingestion_mesh_go_worker",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "SM-077",
        "name": "Bootstrapped SaaS MVP (Node + Postgres + S3)",
        "cat": "Small-Scale Cloud",
        "dir_pattern": "SM-077_bootstrapped_saas_mvp_node_postgres_s3_node_postgres_api",
        "tier": "BALANCED"
    },
    {
        "id": "SM-080",
        "name": "Single-Node Internal Admin Tool with SQLite WAL",
        "cat": "Small-Scale Cloud",
        "dir_pattern": "SM-080_single_node_internal_admin_tool_with_sqlite_wal_node_postgres_api",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "DC-114",
        "name": "64-Node Bare-Metal H100 InfiniBand Cluster",
        "cat": "Data Centre On-Premise",
        "dir_pattern": "DC-114_64_node_bare_metal_h100_infiniband_cluster_python_gpu_vllm",
        "tier": "HIGH_PERFORMANCE"
    },
    {
        "id": "DC-116",
        "name": "Bare-Metal Ceph 2PB NVMe-oF Storage Cluster",
        "cat": "Data Centre On-Premise",
        "dir_pattern": "DC-116_bare_metal_ceph_2pb_nvme_of_storage_cluster_stateful_store_db",
        "tier": "HIGH_PERFORMANCE"
    }
]

def find_problem_dir(pattern):
    return os.path.join(TESTS_ROOT, pattern)

def count_ministack_200_lines():
    if not os.path.exists(MINISTACK_LOG):
        return 0
    count = 0
    with open(MINISTACK_LOG, "r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            if "Status: 200" in line:
                count += 1
    return count

def get_ministack_lines():
    if not os.path.exists(MINISTACK_LOG):
        return []
    with open(MINISTACK_LOG, "r", encoding="utf-8", errors="ignore") as f:
        return f.readlines()

def run_spot_check():
    print("=" * 80)
    print("LTN.ENG AUDITED SPOT-CHECK VERIFICATION (PURE TIER-DRIVEN ROUTING)")
    print(f"Timestamp: {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"Target Binary: {LTN_BIN}")
    print(f"MiniStack Log: {MINISTACK_LOG}")
    print("=" * 80)

    initial_200_count = count_ministack_200_lines()
    print(f"[*] Initial MiniStack 'Status: 200' count: {initial_200_count}")

    # Launch MCP server process over stdio
    proc = subprocess.Popen(
        [LTN_BIN, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=os.environ.copy()
    )

    def rpc(method, params=None, req_id=1):
        payload = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params:
            payload["params"] = params
        proc.stdin.write(json.dumps(payload) + "\n")
        proc.stdin.flush()
        line = proc.stdout.readline()
        if not line:
            return {}
        return json.loads(line.strip())

    # Step 0: Initialize
    init_res = rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "spot-check-auditor", "version": "1.0"}}, 1)
    assert init_res.get("result", {}).get("serverInfo", {}).get("name") == "ltn-eng", f"MCP init failed, got: {init_res}"
    print("[✓] MCP handshake initialized with ltn-eng\n")

    results = []
    deployed_through_ministack = 0

    for idx, prob in enumerate(SPOT_PROBLEMS):
        pid = prob["id"]
        pname = prob["name"]
        cat = prob["cat"]
        tier = prob["tier"]
        pdir = find_problem_dir(prob["dir_pattern"])

        print(f"\n--- [{idx+1}/8] Problem {pid}: {pname} ---")
        print(f"  Category: {cat} | Requested Tier: {tier}")
        print(f"  Path: {pdir}")

        # 1. Analyze
        ana_req = 100 + idx * 10
        ana_res = rpc("tools/call", {"name": "ltn_analyze", "arguments": {"path": pdir}}, ana_req)
        ana_text = ana_res.get("result", {}).get("content", [{}])[0].get("text", "{}")
        ana_data = json.loads(ana_text)

        print(f"  [1. Workload Sizing & Classification]:")
        print(f"      - Workload Kind: {ana_data.get('workload_kind')}")
        print(f"      - CPU Cores    : {ana_data.get('recommended_cpu_cores')}")
        print(f"      - Memory       : {ana_data.get('recommended_ram_mb')} MB")
        print(f"      - GPU Count    : {ana_data.get('recommended_gpu_count')}")
        print(f"      - GPU VRAM     : {ana_data.get('recommended_vram_mb')} MB")
        print(f"      - Description  : {ana_data.get('description')}")

        # 2. Architect
        arch_req = 100 + idx * 10 + 1
        arch_res = rpc("tools/call", {"name": "ltn_architect", "arguments": {"path": pdir}}, arch_req)
        arch_text = arch_res.get("result", {}).get("content", [{}])[0].get("text", "[]")
        plans = json.loads(arch_text)

        print(f"  [2. Architecture & Dynamic Cost Breakdown]:")
        selected_plan = None
        for p in plans:
            is_chosen = (p.get("tier", "").upper() == tier.upper() or (tier.upper() == "HIGH_PERFORMANCE" and p.get("tier") == "HighPerformance") or (tier.upper() == "BALANCED" and p.get("tier") == "Balanced"))
            marker = "==> [SELECTED FOR DEPLOYMENT]" if is_chosen else "   "
            if is_chosen:
                selected_plan = p
            print(f"    {marker} Tier: {p.get('tier'):<16} | Provider: {p.get('provider'):<22} | Cost: ${p.get('estimated_monthly_cost_usd'):,.2f}/mo")

        # 3. Deploy (PURE TIER-DRIVEN ROUTING: NO MANUAL PROVIDER ARGUMENT)
        time.sleep(1.05)
        lines_before = get_ministack_lines()
        count_before = sum(1 for line in lines_before if "Status: 200" in line)

        dep_req = 100 + idx * 10 + 2
        dep_args = {"path": pdir, "tier": tier}

        print(f"  [3. Deployment Invocation & Execution]:")
        print(f"      - MCP Tool Called  : ltn_deploy")
        print(f"      - Exact Call Args  : {json.dumps(dep_args)}  <-- Note: ZERO manual provider override!")
        print(f"      - Plan's Provider  : {selected_plan.get('provider') if selected_plan else 'UNKNOWN'}")

        dep_res = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args}, dep_req)
        dep_text = dep_res.get("result", {}).get("content", [{}])[0].get("text", "{}")
        dep_data = json.loads(dep_text)

        lines_after = get_ministack_lines()
        count_after = sum(1 for line in lines_after if "Status: 200" in line)
        new_lines = lines_after[len(lines_before):]

        is_deployed = dep_data.get("status") == "DEPLOYED"
        routed_ms = dep_data.get("routed_through_ministack", False)
        if is_deployed and routed_ms:
            deployed_through_ministack += 1

        identifier = dep_data.get("instance_id") or dep_data.get("cluster_id") or dep_data.get("resource_group")

        print(f"      - Deployed Provider: {dep_data.get('provider')}")
        print(f"      - Identifier       : {identifier}")
        print(f"      - Endpoint         : {dep_data.get('endpoint')}")
        print(f"      - Routed to MS     : {routed_ms}")
        print(f"      - Actuator Status  : {dep_data.get('actuator_status')}")
        print(f"      - Raw MS Response  : {dep_data.get('raw_response_body')}")
        if new_lines:
            for nl in new_lines:
                print(f"      -> MiniStack RAW Log : {nl.strip()}")
        else:
            print(f"      -> MiniStack RAW Log : (none)")
        if new_lines:
            for nl in new_lines:
                print(f"      -> MiniStack RAW Log : {nl.strip()}")
        else:
            print(f"      -> MiniStack RAW Log : (none)")

        results.append({
            "id": pid,
            "name": pname,
            "category": cat,
            "tier": tier,
            "path": pdir,
            "classification": ana_data,
            "plans": plans,
            "deployment": dep_data,
            "ministack_raw_lines": [nl.strip() for nl in new_lines]
        })

    proc.stdin.close()
    proc.terminate()

    final_200_count = count_ministack_200_lines()
    delta_200 = final_200_count - initial_200_count

    print("\n" + "=" * 80)
    print("  RUNTIME INVARIANT AUDIT VERIFICATION")
    print("=" * 80)
    print(f"  Pre-run MiniStack 200 OK lines   : {initial_200_count}")
    print(f"  Post-run MiniStack 200 OK lines  : {final_200_count}")
    print(f"  Delta MiniStack 200 OK lines     : {delta_200}")
    print(f"  Deployed through MiniStack count : {deployed_through_ministack}")

    invariant_passed = (delta_200 == deployed_through_ministack)
    if invariant_passed:
        print(f"\n[PASSED] Strict Invariant Verified: delta_200 ({delta_200}) == deployed_through_ministack ({deployed_through_ministack})")
    else:
        print(f"\n[FAILED] INVARIANT VIOLATION: delta_200 ({delta_200}) != deployed_through_ministack ({deployed_through_ministack})")
        sys.exit(1)

    summary = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "total_evaluated": len(SPOT_PROBLEMS),
        "initial_ministack_200_count": initial_200_count,
        "final_ministack_200_count": final_200_count,
        "delta_ministack_200_lines": delta_200,
        "deployed_through_ministack_count": deployed_through_ministack,
        "runtime_invariant_satisfied": invariant_passed,
        "spot_check_records": results
    }

    with open(OUT_JSON, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)
    print(f"  Saved detailed audit results to: {OUT_JSON}")
    print("=" * 80)

if __name__ == "__main__":
    run_spot_check()
