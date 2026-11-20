# ltn MCP tools

The `ltn` MCP server (`ltn mcp`, stdio) exposes these tools. Every result is
real: a tool that cannot do its job returns an error, never a made-up success.

Architect
- `ltn_doctor`: check Docker, Z3, state directory, cloud credentials and tools.
- `map_mtl_topology`: real CPU, RAM and GPU inventory of this machine.
- `ltn_analyze`: read a project directory (Dockerfile, optional `ltn.toml`) into an app spec.
- `ltn_plan`: Z3 placement of the app on a registered target within the budget; returns a `plan_id`.
- `ltn_cloud_auth`, `ltn_cloud_catalog`, `ltn_cloud_targets`: AWS credentials check, instance catalog, infrastructure ltn created.

Orchestrator
- `ltn_target_add`, `ltn_targets`, `ltn_target_remove`: Docker hosts ltn may deploy to (local or `ssh://`).
- `ltn_apply`: execute a `plan_id` from `ltn_plan`. Builds, runs, health-checks, keeps the old release on failure.
- `ltn_rollback`: return an app to an earlier release.
- `ltn_destroy`: remove an app; requires `confirm` equal to the app name.

Observer and debugger
- `ltn_status`, `ltn_logs`, `ltn_audit`: live state, container logs, audit trail.
- `ltn_diagnose`: findings with evidence (exit codes, OOM, restart loops, failed probes).
- `ltn_repair`: restart or roll back an unhealthy app.

Rules for agents
- Deploy order: `ltn_doctor`, `ltn_analyze`, `ltn_plan`, `ltn_apply`.
- Never pass secrets as tool arguments; credentials come from the server's environment.
- Cloud VMs are only created when the server runs with `LTN_ALLOW_CLOUD_SPEND=1`.
- Report tool errors as they are.
