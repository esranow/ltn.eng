"""Minimal Google ADK agent that uses ltn's tools over MCP stdio.

UNTESTED: written against the google-adk MCPToolset API but not run as part
of the ltn test suite. The ltn side (`ltn mcp` over stdio) is tested by
tests/e2e/mcp_e2e.py.

Setup:
    pip install google-adk
    export GOOGLE_API_KEY=...            # or Vertex AI credentials
    export LTN_BIN=/path/to/ltn          # default: `ltn` on PATH
    adk run examples/                    # or: adk web
(ADK discovers `root_agent` in this module.)

libz3.dll (or libz3.so) must be on PATH / LD_LIBRARY_PATH for ltn to start.
"""

import os

from google.adk.agents import LlmAgent
from google.adk.tools.mcp_tool.mcp_toolset import MCPToolset, StdioServerParameters

LTN_BIN = os.environ.get("LTN_BIN", "ltn")

ltn_tools = MCPToolset(
    connection_params=StdioServerParameters(
        command=LTN_BIN,
        args=["mcp"],
        # Pass the environment through so Docker, SSH agent and cloud
        # credential chains work; tighten the budget for the agent.
        env={**os.environ, "LTN_MAX_HOURLY_CENTS": os.environ.get("LTN_MAX_HOURLY_CENTS", "50")},
    ),
)

root_agent = LlmAgent(
    name="ltn_devops",
    model="gemini-2.5-flash",
    instruction=(
        "You deploy and operate containerised apps with the ltn tools. "
        "Start with ltn_doctor. To deploy: ltn_analyze, then ltn_plan, then ltn_apply with the plan_id. "
        "Check ltn_status and ltn_logs after every change. When something is unhealthy, call ltn_diagnose "
        "and act on its findings (ltn_repair or ltn_rollback). Never call ltn_destroy unless the user asked "
        "for that app to be removed. Report tool errors to the user verbatim instead of guessing."
    ),
    tools=[ltn_tools],
)
