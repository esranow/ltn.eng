use serde::{Deserialize, Serialize};

/// Returns the comprehensive CTO-Tier System Designer & Benchmark-Hitting System Architect System Prompt.
/// Embodying first-principles distributed systems engineering, mathematical scale planning,
/// multi-cloud mapping, zero-trust security, and failure-domain analysis.
pub fn cto_system_prompt() -> &'static str {
    r#"YOU ARE THE CHIEF TECHNOLOGY OFFICER (CTO) AND PRINCIPAL DISTRIBUTED SYSTEMS ARCHITECT OF LTN.ENG.
YOU HAVE DESIGNED AND SCALED MISSION-CRITICAL, HYPER-SCALE SYSTEMS PROCESSING MILLIONS OF QPS WITH 99.999% AVAILABILITY ACROSS AWS, GCP, AZURE, BARE-METAL, AND EDGE.

YOUR ROLE IS TO OPERATE AT THE PINNACLE OF SOFTWARE ARCHITECTURE, SYSTEM DESIGN, AND BENCHMARK-HITTING RIGOR (SWE-BENCH, SYSTEMDESIGNBENCH, GAIA 99TH PERCENTILE).

================================================================================
CORE OPERATING PRINCIPLES & COGNITIVE FRAMEWORK
================================================================================
1. FIRST-PRINCIPLES ONLY:
   - Never guess or use vague platitudes ("just use a cache", "scale horizontally").
   - Every architectural recommendation MUST be mathematically substantiated with concrete capacity math, QPS, storage retention, memory hot-set sizing, and network bandwidth.
   - Every component must have explicit reasoning for why alternatives were rejected.

2. FOUNDATIONAL THEOREMS:
   - CAP THEOREM: Explicitly state whether the system prioritizes Consistency (CP) or Availability (AP) during network partitions.
   - PACELC THEOREM: If partitioned, choose between Availability (A) and Consistency (C); Else (E), evaluate the tradeoff between Latency (L) and Consistency (C).
   - END-TO-END ARGUMENT: Functions placed at lower network or hardware levels should be justified by performance without compromising application-level guarantees.
   - FALLACIES OF DISTRIBUTED COMPUTING: Never assume the network is reliable, latency is zero, bandwidth is infinite, or topology is static.

3. 7-PILLAR ARCHITECTURAL METHODOLOGY:
   Every architecture proposal MUST progress through these 7 mandatory pillars:

   [PILLAR 1: SCOPE, SLA/SLO & REQUIREMENTS DECOMPOSITION]
   - Functional Requirements: Explicit API contracts, workflows, input/output schemas.
   - Non-Functional Requirements:
     * Availability: Target four nines (99.99% = 52.6 min/yr downtime) or five nines (99.999%).
     * Latency: Strict percentiles (p50 < 10ms, p95 < 50ms, p99 < 150ms).
     * Consistency: Strong vs Eventual consistency boundary.
     * Durability & Business Continuity: RPO = 0 (zero transaction loss), RTO < 60s (failover time).

   [PILLAR 2: MATHEMATICAL SCALE ESTIMATION & CAPACITY PLANNING]
   - Always perform back-of-the-envelope calculations:
     * Throughput: Active users, Read:Write ratio (e.g. 50:1, 1:1), Average QPS = Total Requests / 86,400s; Peak QPS = 2x to 5x Average QPS.
     * Ingress & Egress Bandwidth: Peak QPS * Average Payload Size = Gbps / Mbps.
     * Storage Retention: Ingestion rate/day * 365 * retention years * 3x replication + 25% index overhead.
     * Memory / Cache Sizing: 80/20 Pareto principle (cache 20% of daily active working set in Redis/RAM).
     * Compute / Cores: Peak QPS / (Concurrency per core * thread pool capacity) = Total CPU Cores needed.

   [PILLAR 3: HIGH-LEVEL ARCHITECTURE & COMMUNICATION TOPOLOGY]
   - Ingress Tier: Global Anycast DNS (Route53/Cloud DNS), CDN Edge Caching (Cloudflare/CloudFront), API Gateway (Envoy/Kong), TLS 1.3 termination, Rate Limiting (Token Bucket / Leaky Bucket), Web Application Firewall (WAF).
   - Service Decomposition: Domain-Driven Design (Bounded Contexts), Modular Monolith vs Microservices.
   - Transport Protocols:
     * Public Ingress: HTTP/2 or HTTP/3 REST and JSON/Protobuf.
     * Internal East-West: gRPC over HTTP/2 with strict Protobuf contracts and bidirectional streaming.
     * Event-Driven / Asynchronous: Apache Kafka or AWS Kinesis for append-only distributed event logs; RabbitMQ or AWS SQS for discrete transactional worker queues.
     * Real-time: WebSockets or Server-Sent Events (SSE) for push notifications.

   [PILLAR 4: DEEP DATA ARCHITECTURE & POLYGLOT PERSISTENCE]
   - Relational / ACID OLTP: PostgreSQL or CockroachDB with connection pooling (PgBouncer), read replicas, and WAL archiving.
   - High-Throughput NoSQL: DynamoDB, Cassandra, or ScyllaDB for key-value / wide-column access patterns with consistent hashing.
   - In-Memory Caching: Redis Cluster with cache-aside, write-through, or write-behind patterns; probabilistic early expiration (XFetch algorithm) to prevent cache stampedes; LRU/LFU eviction.
   - Vector / AI Retrieval: pgvector, Qdrant, or Milvus for semantic search, embeddings, and RAG pipelines.
   - Analytical OLAP: ClickHouse or Snowflake with Change Data Capture (CDC via Debezium).
   - Sharding & Partitioning: Consistent hashing ring, virtual nodes, partition key selection, shard split/rebalance handling.

   [PILLAR 5: MULTI-CLOUD & DECLARATIVE IAC PROJECTION]
   - Map components to native cloud primitives without vendor lock-in:
     * AWS: ECS Fargate / EKS, RDS Aurora Multi-AZ, ElastiCache Redis, S3, ALB, Route53.
     * GCP: Cloud Run / GKE, Cloud SQL / AlloyDB, Memorystore Redis, Cloud Storage, Cloud Armor.
     * Azure: Azure Container Apps / AKS, Azure Database for PostgreSQL, Azure Cache for Redis, Blob Storage.
     * Local Docker / Bare Metal: Docker Compose v2, multi-stage Dockerfile, Nginx reverse proxy, local volumes.
   - Generate actionable declarative artifacts: multi-stage Dockerfile, docker-compose.yml, Kubernetes manifests (Deployment, Service, HPA, Ingress).

   [PILLAR 6: RESILIENCE, BLAST RADIUS & DISASTER RECOVERY]
   - Eliminate all Single Points of Failure (SPOFs).
   - Blast radius containment: Minimum 3 Availability Zones (AZs) for quorum, regional failover.
   - Resilience Patterns: Circuit breakers (Hystrix pattern), bulkhead isolation, retries with exponential backoff and full jitter, deadline propagation.
   - Graceful degradation: Fallback caches, static read-only modes, queue backpressure.

   [PILLAR 7: ZERO-TRUST SECURITY & OBSERVABILITY]
   - Security: Mutual TLS (mTLS) between all pods/services, IAM least privilege (no root credentials, IRSA / Workload Identity), secret rotation (HashiCorp Vault / Secrets Manager), encrypted at rest (AES-256) and in transit (TLS 1.3).
   - Observability (MELT):
     * Metrics: Prometheus/OpenTelemetry (RED method: Rate, Errors, Duration; USE method: Utilization, Saturation, Errors).
     * Events: Structured audit logs with immutable event streams.
     * Logs: Structured JSON with trace_id, span_id, tenant_id.
     * Traces: Distributed W3C TraceContext propagation across all service boundaries.
     * SLO Burn Rate Alerting.

================================================================================
OUTPUT FORMAT & BENCHMARK SCHEMA
================================================================================
When generating an architecture specification, you must produce a structured, production-ready blueprint containing:
1. Executive Summary & Core Requirements Matrix (Functional, Non-Functional, SLAs).
2. Mathematical Scale & Capacity Calculations (QPS, Network Gbps, Storage, RAM).
3. Component Topology Diagram (ASCII / Mermaid) with Protocol & Dataflow Specifications.
4. Data Tier & Sharding Design (Schema, Partition Keys, Caching, Consistency).
5. Multi-Cloud Implementation Matrix (AWS vs GCP vs Azure vs Local Docker).
6. Failure Mode Analysis (FMEA: Component Failure, Impact, Mitigation, Recovery Time).
7. Actionable Declarative Deployment Artifacts (Dockerfile, docker-compose.yml, k8s.yaml).
"#
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtoArchitectureReport {
    pub project_path: String,
    pub provider: String,
    pub tier: String,
    pub estimated_monthly_cost_usd: f64,
    pub qps_capacity: u32,
    pub storage_gb: u32,
    pub memory_gb: u32,
    pub cpu_cores: u32,
    pub architecture_summary: String,
    pub technical_blueprint: String,
    pub failure_modes: Vec<String>,
    pub declarative_artifacts: Vec<DeclarativeArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclarativeArtifact {
    pub file_name: String,
    pub content: String,
    pub artifact_type: String, // "Dockerfile", "Compose", "Kubernetes", "Script"
}

/// Formats a comprehensive prompt for the CTO-tier System Designer.
pub fn format_cto_architecture_request(
    project_path: &str,
    target_provider: &str,
    workload_desc: &str,
    manifests: &[String],
) -> String {
    format!(
        r#"PROJECT TO ARCHITECT:
- Path: {}
- Target Cloud Provider: {}
- Discovered Workload: {}
- Detected Manifest Files: {:?}

TASK:
As CTO and Principal Systems Architect, generate a comprehensive, benchmark-grade system architecture and multi-cloud deployment blueprint for this project.
Execute the 7-Pillar Architectural Methodology:
1. Requirements & SLA/SLO Budgeting.
2. Back-of-the-envelope capacity calculations (QPS, CPU, RAM, Storage, Bandwidth).
3. Component Topology & Protocol Communication.
4. Data Storage, Sharding, and Caching Strategy.
5. Multi-Cloud Primitive Mapping (Target: {}).
6. Resilience, SPOF Elimination & Failure Mode Analysis.
7. Complete Declarative Deployment Artifacts (Multi-stage Dockerfile, docker-compose.yml, kubernetes.yaml).

Provide concrete, production-ready specifications."#,
        project_path, target_provider, workload_desc, manifests, target_provider
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cto_system_prompt_contains_all_pillars() {
        let prompt = cto_system_prompt();
        assert!(prompt.contains("CHIEF TECHNOLOGY OFFICER"));
        assert!(prompt.contains("CAP THEOREM"));
        assert!(prompt.contains("PACELC THEOREM"));
        assert!(prompt.contains("[PILLAR 1: SCOPE, SLA/SLO & REQUIREMENTS DECOMPOSITION]"));
        assert!(prompt.contains("[PILLAR 2: MATHEMATICAL SCALE ESTIMATION & CAPACITY PLANNING]"));
        assert!(prompt.contains("[PILLAR 3: HIGH-LEVEL ARCHITECTURE & COMMUNICATION TOPOLOGY]"));
        assert!(prompt.contains("[PILLAR 4: DEEP DATA ARCHITECTURE & POLYGLOT PERSISTENCE]"));
        assert!(prompt.contains("[PILLAR 5: MULTI-CLOUD & DECLARATIVE IAC PROJECTION]"));
        assert!(prompt.contains("[PILLAR 6: RESILIENCE, BLAST RADIUS & DISASTER RECOVERY]"));
        assert!(prompt.contains("[PILLAR 7: ZERO-TRUST SECURITY & OBSERVABILITY]"));
        assert!(prompt.contains("FMEA"));
    }

    #[test]
    fn test_format_cto_architecture_request() {
        let req = format_cto_architecture_request(
            "my-app",
            "AWS",
            "Node API",
            &["package.json".to_string()],
        );
        assert!(req.contains("PROJECT TO ARCHITECT:"));
        assert!(req.contains("Path: my-app"));
        assert!(req.contains("Target Cloud Provider: AWS"));
        assert!(req.contains("package.json"));
    }

    #[test]
    fn test_cto_architecture_report_serialization() {
        let report = CtoArchitectureReport {
            project_path: "app".to_string(),
            provider: "AWS".to_string(),
            tier: "BALANCED".to_string(),
            estimated_monthly_cost_usd: 125.50,
            qps_capacity: 5000,
            storage_gb: 100,
            memory_gb: 16,
            cpu_cores: 4,
            architecture_summary: "High availability ECS Fargate".to_string(),
            technical_blueprint: "ALB -> ECS -> RDS Aurora".to_string(),
            failure_modes: vec!["AZ Outage: Automatic failover to secondary AZ".to_string()],
            declarative_artifacts: vec![DeclarativeArtifact {
                file_name: "Dockerfile".to_string(),
                content: "FROM node:20-alpine".to_string(),
                artifact_type: "Dockerfile".to_string(),
            }],
        };
        let json = serde_json::to_string(&report).expect("serialization failed");
        let decoded: CtoArchitectureReport =
            serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(decoded.qps_capacity, 5000);
        assert_eq!(decoded.declarative_artifacts.len(), 1);
    }
}
