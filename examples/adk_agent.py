"""Minimal Google ADK agent that uses ltn's tools over MCP stdio.

Connection tested with google-adk 2.15.0 by tests/e2e/adk_connect.py: ADK
starts `ltn mcp`, lists the tools and calls them. The model loop itself (an
LLM choosing the tools) has not been run in the ltn test suite because it
needs a model API key.

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
from google.adk.tools.mcp_tool.mcp_session_manager import StdioConnectionParams
from google.adk.tools.mcp_tool.mcp_toolset import McpToolset
from mcp import StdioServerParameters

LTN_BIN = os.environ.get("LTN_BIN", "ltn")

ltn_tools = McpToolset(
    connection_params=StdioConnectionParams(
        server_params=StdioServerParameters(
            command=LTN_BIN,
            args=["mcp"],
            # Pass the environment through so Docker, SSH agent and cloud
            # credential chains work; tighten the budget for the agent.
            env={**os.environ, "LTN_MAX_HOURLY_CENTS": os.environ.get("LTN_MAX_HOURLY_CENTS", "50")},
        ),
        # Seconds ADK waits for each MCP request. ltn_apply builds an image and
        # waits for health, so this must be long.
        timeout=900.0,
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
