#!/usr/bin/env python3
"""
LTN.ENG MCP Integration Verification Test Suite for Antigravity
Tests JSON-RPC 2.0 handshake, tool enumeration, and dispatching all 9 tools over stdio.
"""

import subprocess
import json
import sys
import os

LTN_BIN = os.path.normpath(os.environ.get("LTN_EXE", r"D:\ltn-eng-workspace\target\debug\ltn.exe"))
FIXTURE_PATH = os.path.normpath(r"D:\ltn-eng-workspace\tests\fixtures\node_postgres_api")

if not os.path.exists(LTN_BIN):
    print(f"[ERROR] Executable not found at {LTN_BIN}")
    sys.exit(1)

print(f"[*] Testing LTN.ENG MCP binary at: {LTN_BIN}")
proc = subprocess.Popen(
    [LTN_BIN, "mcp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True
)

def rpc(method, params=None, req_id=1):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        req["params"] = params
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)

failed_tests = 0

# 1. Initialize
try:
    init_res = rpc("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "antigravity", "version": "2.0"}
    }, 1)
    server_info = init_res.get("result", {}).get("serverInfo", {})
    print(f"[PASS] MCP Handshake: Server '{server_info.get('name')}' v{server_info.get('version')}")
except Exception as e:
    print(f"[FAIL] MCP Handshake failed: {e}")
    failed_tests += 1

# 2. Tools List
try:
    tools_res = rpc("tools/list", req_id=2)
    tools = tools_res.get("result", {}).get("tools", [])
    print(f"[PASS] Tools Discovery: Found {len(tools)} tools")
    for t in tools:
        print(f"       - {t['name']}: {t['description']}")
except Exception as e:
    print(f"[FAIL] Tool list query failed: {e}")
    failed_tests += 1

# 3. Test Dispatches
test_cases = [
    ("ltn_doctor", {}),
    ("map_mtl_topology", {}),
    ("ltn_analyze", {"path": FIXTURE_PATH}),
    ("ltn_architect", {"path": FIXTURE_PATH}),
    ("ltn_status", {}),
    ("ltn_snapshot", {}),
    ("ltn_rollback", {}),
    ("ltn_config", {"model": "gemini-2.5"}),
    ("ltn_deploy", {"path": FIXTURE_PATH, "tier": "BALANCED"})
]

print("\n[*] Testing Tool Execution over MCP stdio:")
for req_id, (tool_name, args) in enumerate(test_cases, start=3):
    try:
        call_res = rpc("tools/call", {"name": tool_name, "arguments": args}, req_id)
        is_error = call_res.get("result", {}).get("isError", True)
        if not is_error and "result" in call_res and "content" in call_res["result"]:
            print(f"  [PASS] {tool_name}")
        else:
            print(f"  [FAIL] {tool_name}: {call_res}")
            failed_tests += 1
    except Exception as e:
        print(f"  [FAIL] {tool_name} error: {e}")
        failed_tests += 1

proc.stdin.close()
proc.terminate()

print("\n" + "="*50)
if failed_tests == 0:
    print("[SUCCESS] All MCP tests passed successfully!")
    sys.exit(0)
else:
    print(f"[WARNING] {failed_tests} MCP test(s) encountered issues.")
    sys.exit(failed_tests)
