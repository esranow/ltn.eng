"""Check that Google ADK can drive ltn over MCP, without calling any LLM.

Imports examples/adk_agent.py (so the example itself is exercised), lets ADK's
MCP toolset start `ltn mcp`, lists the tools and calls two read-only ones
through ADK's own tool objects. No model is contacted and no API key is needed.

    pip install google-adk
    LTN_BIN=/path/to/ltn python tests/e2e/adk_connect.py

The Z3 DLL must be on PATH for ltn to start.
"""

import asyncio
import importlib.util
import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = {
    "ltn_doctor", "map_mtl_topology", "ltn_analyze", "ltn_plan", "ltn_apply", "ltn_rollback",
    "ltn_destroy", "ltn_targets", "ltn_target_add", "ltn_target_remove", "ltn_status", "ltn_logs",
    "ltn_diagnose", "ltn_repair", "ltn_audit", "ltn_cloud_auth", "ltn_cloud_catalog", "ltn_cloud_targets",
}


def text_of(result):
    """Pull the JSON text out of whatever shape this ADK version returns."""
    if hasattr(result, "model_dump"):
        result = result.model_dump()
    if isinstance(result, dict) and "content" in result:
        parts = result["content"]
        if parts and isinstance(parts[0], dict) and "text" in parts[0]:
            return bool(result.get("isError")), parts[0]["text"]
    return False, json.dumps(result, default=str)


async def main():
    os.environ.setdefault("LTN_HOME", tempfile.mkdtemp(prefix="ltn-adk-"))
    os.environ.setdefault("LTN_WATCH", "0")
    spec = importlib.util.spec_from_file_location("adk_agent", ROOT / "examples" / "adk_agent.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    agent, toolset = mod.root_agent, mod.ltn_tools
    print(f"ok  example imported; agent={agent.name} model={agent.model}")
    failed = 0
    try:
        tools = await toolset.get_tools()
        names = {t.name for t in tools}
        missing, extra = EXPECTED - names, names - EXPECTED
        if missing or extra:
            failed += 1
            print(f"FAIL tool list: missing={sorted(missing)} extra={sorted(extra)}")
        else:
            print(f"ok  ADK listed {len(names)} ltn tools")
        by_name = {t.name: t for t in tools}
        for name in ("ltn_doctor", "map_mtl_topology"):
            is_err, text = text_of(await by_name[name].run_async(args={}, tool_context=None))
            data = json.loads(text) if text.lstrip().startswith("{") else None
            if is_err or data is None or set(data) == {"error"}:
                failed += 1
                print(f"FAIL {name}: {text[:300]}")
            else:
                print(f"ok  {name} via ADK -> keys {sorted(data)[:8]}")
    finally:
        await toolset.close()
    print("ADK CONNECT:", "FAILED" if failed else "PASSED")
    return failed


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
