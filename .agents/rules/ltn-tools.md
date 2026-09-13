# LTN.ENG MCP Integration Guidelines

The `ltn-eng` MCP server is integrated and exposes the following tools:

- `ltn_doctor`: Run diagnostic check on environment, container sandbox, and cloud provider authentication.
- `ltn_analyze`: Analyze project workload requirements (CPU cores, RAM MB, VRAM, files) given a project root directory `path`.
- `ltn_architect`: Generate 3 architecture deployment options (Cost-Effective, Balanced, High-Performance) for a project directory `path`.
- `ltn_deploy`: Deploy project workload with formal Z3 SMT validation and agent harness verification (tier: `CHEAPEST`, `BALANCED`, `HIGH_PERFORMANCE`).
- `ltn_status`: Query operational telemetry, active socket probes, and health metrics from `endpoint` (defaults to `http://localhost:8080`).
- `ltn_snapshot`: Save atomic system state snapshot with SHA-256 hash.
- `ltn_rollback`: Roll back system state to previous verified snapshot.
- `ltn_config`: Manage configuration settings, cloud provider, and LLM model routing.
- `map_mtl_topology`: Discover bare-metal CPU, RAM, and GPU VRAM topology.
