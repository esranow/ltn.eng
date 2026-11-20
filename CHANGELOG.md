# Changelog

## 0.1.0 (unreleased)

First release of the real MCP runtime. `ltn mcp` serves 18 tools over stdio.

- Architect: `ltn_doctor`, `map_mtl_topology`, `ltn_analyze`, `ltn_plan` (Z3
  placement within a budget, with an unsat core when nothing fits).
- Orchestrator: `ltn_targets`, `ltn_target_add`, `ltn_target_remove`,
  `ltn_apply` (build, run, health-check, keep the previous release on
  failure), `ltn_rollback`, `ltn_destroy`.
- Observer and debugger: `ltn_status`, `ltn_logs`, `ltn_audit`,
  `ltn_diagnose`, `ltn_repair`, and a background watcher that restarts or
  rolls back unhealthy apps.
- Cloud: `ltn_cloud_auth`, `ltn_cloud_catalog`, `ltn_cloud_targets`; AWS VMs
  through OpenTofu, refused unless `LTN_ALLOW_CLOUD_SPEND=1`.
- Every Docker CLI call has a time limit (`LTN_DOCKER_TIMEOUT_S`,
  `LTN_DOCKER_BUILD_TIMEOUT_S`), so a stalled daemon or SSH link returns an
  error instead of hanging the server.
- On Windows, external tools such as `aws.cmd` are found through `PATHEXT`.

See "Status and limits" in README.md for what has and has not been verified.
