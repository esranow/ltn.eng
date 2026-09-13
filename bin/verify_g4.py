import subprocess
import json
import os
import shutil

LTN_BIN = r"D:\ltn-eng-workspace\target\debug\ltn.exe"
TOPOLOGY_FILE = r"D:\.ltn-graph\nodes.json"
TOPOLOGY_BACKUP = r"D:\.ltn-graph\nodes.json.bak"
DC116_PATH = r"D:\ltn-eng-workspace\tests\eval_150_system_design\DC-116_bare_metal_ceph_2pb_nvme_of_storage_cluster_stateful_store_db"

def call_ltn_deploy(topology_present=True):
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

    # MCP Handshake
    rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "g4-verifier", "version": "1.0"}}, 1)

    # Tool call: ltn_deploy for DC-116 Balanced with bare_metal=True
    dep_args = {
        "path": DC116_PATH,
        "tier": "BALANCED",
        "bare_metal": True
    }
    res = rpc("tools/call", {"name": "ltn_deploy", "arguments": dep_args}, 2)
    proc.stdin.close()
    proc.terminate()

    receipt_str = res.get("result", {}).get("content", [{}])[0].get("text", "{}")
    return json.loads(receipt_str)

def main():
    print("================================================================================")
    print("RUN 1: DC-116 Balanced Deployment WITH Topology File Present (2 Nodes Expected)")
    print("================================================================================")
    assert os.path.exists(TOPOLOGY_FILE), f"Topology file {TOPOLOGY_FILE} must exist for Run 1"
    receipt_with_topo = call_ltn_deploy(topology_present=True)
    print(json.dumps(receipt_with_topo, indent=2))

    print("\n================================================================================")
    print("RUN 2: DC-116 Balanced Deployment WITHOUT Topology File (Fallback Mismatch)")
    print("================================================================================")
    shutil.move(TOPOLOGY_FILE, TOPOLOGY_BACKUP)
    try:
        receipt_without_topo = call_ltn_deploy(topology_present=False)
        print(json.dumps(receipt_without_topo, indent=2))
    finally:
        shutil.move(TOPOLOGY_BACKUP, TOPOLOGY_FILE)
        print("\n[+] Restored D:/.ltn-graph/nodes.json successfully")

if __name__ == "__main__":
    main()
