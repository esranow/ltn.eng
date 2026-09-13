import subprocess
import json
import os
import sys

LTN_BIN = r"D:\ltn-eng-workspace\target\debug\ltn.exe"

def run_e2e():
    env = os.environ.copy()
    env["AWS_ENDPOINT_URL"] = "http://127.0.0.1:4566"
    env["AZURE_ENDPOINT_URL"] = "http://127.0.0.1:4566"
    env["GCP_ENDPOINT_URL"] = "http://127.0.0.1:4566"
    env["AWS_ACCESS_KEY_ID"] = "mock_key"
    env["AWS_SECRET_ACCESS_KEY"] = "mock_secret"

    proc = subprocess.Popen(
        [LTN_BIN, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env=env
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

    # Step 0: MCP Handshake
    init_res = rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "e2e-tester", "version": "1.0"}}, 1)
    print("=== MCP Handshake ===")
    print("Server Info:", json.dumps(init_res.get("result", {}).get("serverInfo", {})))

    # Test 1: MT-004 (Edge Wasm -> Fastly Compute -> AWS Cloud Edge Fallback)
    print("\n" + "=" * 80)
    print("TEST 1: MT-004 Fastly Edge Workload -> AWS (Cloud Edge Fallback)")
    print("=" * 80)
    mt004_path = r"D:\ltn-eng-workspace\tests\eval_150_system_design\MT-004_multi_tenant_kms_envelope_encryption_rust_wasm"
    dep_args_1 = {"path": mt004_path, "tier": "BALANCED"}
    print(f"Tool Call: ltn_deploy({json.dumps(dep_args_1)})")
    res_1 = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args_1}, 2)
    receipt_1 = json.loads(res_1.get("result", {}).get("content", [{}])[0].get("text", "{}"))
    print("Receipt Output:")
    print(json.dumps(receipt_1, indent=2))

    # Test 2: SC-039 (Edge Wasm -> Cloudflare Workers -> AWS Cloud Edge Fallback)
    print("\n" + "=" * 80)
    print("TEST 2: SC-039 Cloudflare Edge Workload -> AWS (Cloud Edge Fallback)")
    print("=" * 80)
    sc039_path = r"D:\ltn-eng-workspace\tests\eval_150_system_design\SC-039_ultra_low_latency_order_matching_engine_rust_wasm"
    dep_args_2 = {"path": sc039_path, "tier": "HIGH_PERFORMANCE"}
    print(f"Tool Call: ltn_deploy({json.dumps(dep_args_2)})")
    res_2 = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args_2}, 3)
    receipt_2 = json.loads(res_2.get("result", {}).get("content", [{}])[0].get("text", "{}"))
    print("Receipt Output:")
    print(json.dumps(receipt_2, indent=2))

    # Test 3: DC-116 Bare-Metal Ceph (Stateful Storage -> BARE_METAL_CEPH)
    print("\n" + "=" * 80)
    print("TEST 3: DC-116 Bare-Metal Ceph Deployment (Live BareMetalActuator)")
    print("=" * 80)
    dc116_path = r"D:\ltn-eng-workspace\tests\eval_150_system_design\DC-116_bare_metal_ceph_2pb_nvme_of_storage_cluster_stateful_store_db"
    dep_args_3 = {"path": dc116_path, "tier": "BALANCED", "bare_metal": True}
    print(f"Tool Call: ltn_deploy({json.dumps(dep_args_3)})")
    res_3 = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args_3}, 4)
    receipt_3 = json.loads(res_3.get("result", {}).get("content", [{}])[0].get("text", "{}"))
    print("Receipt Output:")
    print(json.dumps(receipt_3, indent=2))

    # Test 4: DC-114 Bare-Metal HPC (GPU Inference -> BARE_METAL_SLURM_INFINIBAND)
    print("\n" + "=" * 80)
    print("TEST 4: DC-114 Bare-Metal HPC Slurm InfiniBand Deployment")
    print("=" * 80)
    dc114_path = r"D:\ltn-eng-workspace\tests\eval_150_system_design\DC-114_64_node_bare_metal_h100_infiniband_cluster_python_gpu_vllm"
    dep_args_4 = {"path": dc114_path, "tier": "HIGH_PERFORMANCE", "bare_metal": True}
    print(f"Tool Call: ltn_deploy({json.dumps(dep_args_4)})")
    res_4 = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args_4}, 5)
    receipt_4 = json.loads(res_4.get("result", {}).get("content", [{}])[0].get("text", "{}"))
    print("Receipt Output:")
    print(json.dumps(receipt_4, indent=2))

    proc.stdin.close()
    proc.terminate()

if __name__ == "__main__":
    run_e2e()
