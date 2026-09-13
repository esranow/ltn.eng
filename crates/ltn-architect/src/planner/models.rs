use super::super::classifier::ProjectClassification;
use ltn_core::ir::WorkloadKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanTier {
    Cheapest,
    Balanced,
    HighPerformance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanNode {
    pub name: String,
    pub role: String,
    pub image: String,
    pub ports: Vec<String>,
    pub environment: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturePlan {
    pub tier: PlanTier,
    pub provider: String,
    pub nodes: Vec<PlanNode>,
    pub estimated_monthly_cost_usd: f64,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProviderRateInfo {
    pub cpu_rate_per_core: f64,
    pub ram_rate_per_gb: f64,
    pub gpu_rate_per_unit: f64,
    pub base_fixed_cost: f64,
    pub min_tier_floor: f64,
}

pub struct ArchitecturePlanner;

impl ArchitecturePlanner {
    pub fn get_provider_rate(provider: &str) -> ProviderRateInfo {
        let p = provider.to_uppercase();
        if p.starts_with("LOCAL") || p == "BARE_METAL" {
            ProviderRateInfo {
                cpu_rate_per_core: 0.0,
                ram_rate_per_gb: 0.0,
                gpu_rate_per_unit: 0.0,
                base_fixed_cost: 0.0,
                min_tier_floor: 0.0,
            }
        } else if p.starts_with("BARE_METAL") {
            // Bare-Metal Datacenter Infrastructure: amortized hardware, rack co-lo, power, cooling, network fabric
            let (fixed, fabric_overhead) = if p.contains("SLURM") || p.contains("NVME") || p.contains("NUMA") {
                (120.0, 120.0)
            } else {
                (45.0, 45.0)
            };
            ProviderRateInfo {
                cpu_rate_per_core: 5.00,
                ram_rate_per_gb: 0.75,
                gpu_rate_per_unit: 450.0,
                base_fixed_cost: fixed,
                min_tier_floor: fabric_overhead,
            }
        } else if p.contains("FASTLY") {
            // Fastly Compute@Edge: per-request/per-core economics, low memory footprint
            ProviderRateInfo {
                cpu_rate_per_core: 12.00,
                ram_rate_per_gb: 1.80,
                gpu_rate_per_unit: 0.0,
                base_fixed_cost: 15.0,
                min_tier_floor: 15.0,
            }
        } else if p.contains("CLOUDFLARE") {
            // Cloudflare Workers / Pages: hyper-efficient edge pricing
            ProviderRateInfo {
                cpu_rate_per_core: 10.00,
                ram_rate_per_gb: 1.50,
                gpu_rate_per_unit: 0.0,
                base_fixed_cost: 10.0,
                min_tier_floor: 10.0,
            }
        } else if p.contains("DIGITAL_OCEAN") || p.contains("DIGITALOCEAN") {
            // DigitalOcean: budget VPS droplets and managed DBs
            ProviderRateInfo {
                cpu_rate_per_core: 18.00,
                ram_rate_per_gb: 2.50,
                gpu_rate_per_unit: 1200.0,
                base_fixed_cost: 0.0,
                min_tier_floor: 12.0,
            }
        } else if p.contains("GCP") {
            // Google Cloud Platform: sustained usage discounts, TPU/GPU competitive rates
            ProviderRateInfo {
                cpu_rate_per_core: 22.50,
                ram_rate_per_gb: 3.10,
                gpu_rate_per_unit: 1480.0,
                base_fixed_cost: 0.0,
                min_tier_floor: 15.0,
            }
        } else if p.contains("AZURE") {
            // Microsoft Azure: enterprise pricing tiers
            ProviderRateInfo {
                cpu_rate_per_core: 25.50,
                ram_rate_per_gb: 3.75,
                gpu_rate_per_unit: 1680.0,
                base_fixed_cost: 0.0,
                min_tier_floor: 18.0,
            }
        } else {
            // AWS (default): standard EC2/Fargate/RDS on-demand rates
            ProviderRateInfo {
                cpu_rate_per_core: 24.00,
                ram_rate_per_gb: 3.50,
                gpu_rate_per_unit: 1606.0,
                base_fixed_cost: 0.0,
                min_tier_floor: 15.0,
            }
        }
    }

    pub fn compute_tier_cost(
        classification: &ProjectClassification,
        tier: &PlanTier,
        provider: &str,
    ) -> f64 {
        let rate = Self::get_provider_rate(provider);
        if rate.cpu_rate_per_core == 0.0 && rate.ram_rate_per_gb == 0.0 && rate.gpu_rate_per_unit == 0.0 {
            return 0.0;
        }

        let cpu = classification.recommended_cpu_cores as f64;
        let ram_gb = (classification.recommended_ram_mb as f64) / 1024.0;
        let gpus = classification.recommended_gpu_count as f64;

        let compute_base = (cpu * rate.cpu_rate_per_core) + (ram_gb * rate.ram_rate_per_gb);
        let gpu_base = gpus * rate.gpu_rate_per_unit;
        let total_base = compute_base + gpu_base + rate.base_fixed_cost;

        match tier {
            PlanTier::Cheapest => {
                (total_base * 0.35).max(rate.min_tier_floor).round()
            }
            PlanTier::Balanced => {
                total_base.max(rate.min_tier_floor).round()
            }
            PlanTier::HighPerformance => {
                let multiplier = if provider.to_uppercase().contains("GCP") {
                    1.75
                } else if provider.to_uppercase().contains("AZURE") {
                    1.90
                } else {
                    1.85
                };
                let overhead = if gpus > 0.0 { 100.0 } else { 35.0 };
                ((total_base * multiplier) + overhead).max(rate.min_tier_floor * 2.0).round()
            }
        }
    }

    pub fn generate_plans(classification: &ProjectClassification) -> Vec<ArchitecturePlan> {
        let mut plans = match classification.workload_kind {
            WorkloadKind::PythonVllmGpu => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "inference-node".to_string(),
                        role: "gpu-worker".to_string(),
                        image: "vllm/vllm-openai:latest".to_string(),
                        ports: vec!["8000:8000".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Single local GPU container setup for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "AWS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "inference-node".to_string(),
                            role: "gpu-worker".to_string(),
                            image: "vllm/vllm-openai:latest".to_string(),
                            ports: vec!["8000:8000".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "gateway-node".to_string(),
                            role: "load-balancer".to_string(),
                            image: "nginx:alpine".to_string(),
                            ports: vec!["443:443".to_string(), "80:80".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 125.0,
                    description: format!("Dedicated AWS EC2 GPU instance with Nginx ingress for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "GCP_GPU_CLUSTER".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "inference-node-01".to_string(),
                            role: "gpu-worker-primary".to_string(),
                            image: "vllm/vllm-openai:latest".to_string(),
                            ports: vec!["8000:8000".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "inference-node-02".to_string(),
                            role: "gpu-worker-secondary".to_string(),
                            image: "vllm/vllm-openai:latest".to_string(),
                            ports: vec!["8001:8000".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "gateway-node".to_string(),
                            role: "load-balancer".to_string(),
                            image: "nginx:alpine".to_string(),
                            ports: vec!["443:443".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 580.0,
                    description: format!("Distributed GCP multi-GPU cluster with tensor parallelism for {}", classification.description),
                },
            ],
            WorkloadKind::StaticWeb => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "static-app".to_string(),
                        role: "web-server".to_string(),
                        image: "nginx:alpine".to_string(),
                        ports: vec!["80:80".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Local containerized static web server for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "CLOUDFLARE_PAGES".to_string(),
                    nodes: vec![PlanNode {
                        name: "cdn-edge-distribution".to_string(),
                        role: "edge-cdn".to_string(),
                        image: "cloudflare/pages-runtime:latest".to_string(),
                        ports: vec!["443:443".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 20.0,
                    description: format!("Global CDN edge static hosting with automated SSL for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "AWS_CLOUDFRONT_S3".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "s3-origin".to_string(),
                            role: "origin-storage".to_string(),
                            image: "amazon/aws-cli:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "cloudfront-edge".to_string(),
                            role: "edge-cache".to_string(),
                            image: "aws/cloudfront-agent:latest".to_string(),
                            ports: vec!["443:443".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 65.0,
                    description: format!("Multi-region S3 bucket origin backed by CloudFront edge POPs for {}", classification.description),
                },
            ],
            WorkloadKind::RustWasm => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_WASMER".to_string(),
                    nodes: vec![PlanNode {
                        name: "wasm-node".to_string(),
                        role: "edge-worker".to_string(),
                        image: "wasmer/runtime:latest".to_string(),
                        ports: vec!["8080:8080".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Standalone Wasmer local runtime for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "FASTLY_COMPUTE".to_string(),
                    nodes: vec![PlanNode {
                        name: "fastly-edge-wasm".to_string(),
                        role: "edge-compute".to_string(),
                        image: "fastly/viceroy:latest".to_string(),
                        ports: vec!["443:443".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 25.0,
                    description: format!("Serverless Wasm edge compute on Fastly POPs for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "CLOUDFLARE_WORKERS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "cf-wasm-core".to_string(),
                            role: "edge-compute".to_string(),
                            image: "cloudflare/workerd:latest".to_string(),
                            ports: vec!["443:443".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "kv-storage-node".to_string(),
                            role: "edge-storage".to_string(),
                            image: "cloudflare/workers-kv:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 85.0,
                    description: format!("Enterprise global WebAssembly edge mesh with distributed Workers KV for {}", classification.description),
                },
            ],
            WorkloadKind::GoWorker => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "go-worker".to_string(),
                        role: "worker".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec![],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Single Go background worker container for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "DIGITAL_OCEAN".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "go-worker".to_string(),
                            role: "worker".to_string(),
                            image: "go-worker:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "redis-broker".to_string(),
                            role: "queue".to_string(),
                            image: "redis:7-alpine".to_string(),
                            ports: vec!["6379:6379".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 30.0,
                    description: format!("Managed Redis message broker with Go consumer workers for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "AWS_ECS_FARGATE".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "worker-pool".to_string(),
                            role: "auto-scaling-workers".to_string(),
                            image: "go-worker:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "sqs-dlq-system".to_string(),
                            role: "managed-queue".to_string(),
                            image: "amazon/sqs-local:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 150.0,
                    description: format!("Auto-scaling Fargate Go worker cluster backed by SQS for {}", classification.description),
                },
            ],
            WorkloadKind::StatefulStore => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "db-node".to_string(),
                        role: "primary-store".to_string(),
                        image: "postgres:16-alpine".to_string(),
                        ports: vec!["5432:5432".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Single container stateful store with local volume mount for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "AWS_RDS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "rds-postgres-primary".to_string(),
                            role: "managed-db-primary".to_string(),
                            image: "aws/rds-postgres:16".to_string(),
                            ports: vec!["5432:5432".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "rds-postgres-replica".to_string(),
                            role: "read-replica".to_string(),
                            image: "aws/rds-postgres:16".to_string(),
                            ports: vec!["5433:5432".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 75.0,
                    description: format!("Managed Multi-AZ RDS Postgres cluster with Read Replica for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "GCP_SPANNER".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "spanner-node-leader".to_string(),
                            role: "distributed-consensus".to_string(),
                            image: "gcp/spanner-emulator:latest".to_string(),
                            ports: vec!["9010:9010".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "spanner-node-witness".to_string(),
                            role: "consensus-witness".to_string(),
                            image: "gcp/spanner-emulator:latest".to_string(),
                            ports: vec!["9020:9010".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 420.0,
                    description: format!("Global synchronous replicated multi-region Spanner cluster for {}", classification.description),
                },
            ],
            WorkloadKind::NodePostgresApi => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "app-node".to_string(),
                        role: "primary".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec!["8080:8080".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Single-container cost-optimized setup for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "AWS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "app-node".to_string(),
                            role: "app-server".to_string(),
                            image: "app:latest".to_string(),
                            ports: vec!["80:8080".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "db-node".to_string(),
                            role: "database".to_string(),
                            image: "postgres:16-alpine".to_string(),
                            ports: vec!["5432:5432".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 35.0,
                    description: format!("High-availability 2-tier setup for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "AZURE_AKS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "api-cluster".to_string(),
                            role: "api-server-pool".to_string(),
                            image: "node:20-alpine".to_string(),
                            ports: vec!["3000:3000".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "db-cluster".to_string(),
                            role: "azure-postgres-flexible".to_string(),
                            image: "postgres:16-alpine".to_string(),
                            ports: vec!["5432:5432".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "azure-app-gateway".to_string(),
                            role: "application-gateway-waf".to_string(),
                            image: "nginx:alpine".to_string(),
                            ports: vec!["443:443".to_string(), "80:80".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 210.0,
                    description: format!("Enterprise Azure Kubernetes Service (AKS) cluster with PostgreSQL Flexible Server and Azure Application Gateway WAF for {}", classification.description),
                },
            ],
            WorkloadKind::GenericContainer => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "LOCAL_DOCKER".to_string(),
                    nodes: vec![PlanNode {
                        name: "app-node".to_string(),
                        role: "primary".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec!["8080:8080".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: 0.0,
                    description: format!("Single-container cost-optimized setup for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "AWS".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "app-node".to_string(),
                            role: "app-server".to_string(),
                            image: "app:latest".to_string(),
                            ports: vec!["80:8080".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "db-node".to_string(),
                            role: "database".to_string(),
                            image: "postgres:16-alpine".to_string(),
                            ports: vec!["5432:5432".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 35.0,
                    description: format!("High-availability 2-tier setup for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "GCP_GPU_CLUSTER".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "inference-node".to_string(),
                            role: "gpu-worker".to_string(),
                            image: "vllm/vllm-openai:latest".to_string(),
                            ports: vec!["8000:8000".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "gateway-node".to_string(),
                            role: "load-balancer".to_string(),
                            image: "nginx:alpine".to_string(),
                            ports: vec!["443:443".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: 280.0,
                    description: format!("Distributed GPU-accelerated cluster for {}", classification.description),
                },
            ],
        };

        for plan in &mut plans {
            plan.estimated_monthly_cost_usd = Self::compute_tier_cost(classification, &plan.tier, &plan.provider);
        }

        plans
    }

    pub fn generate_bare_metal_plans(classification: &ProjectClassification) -> Vec<ArchitecturePlan> {
        match classification.workload_kind {
            WorkloadKind::PythonVllmGpu => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "BARE_METAL".to_string(),
                    nodes: vec![PlanNode {
                        name: "bm-gpu-node-01".to_string(),
                        role: "direct-pcie-passthrough".to_string(),
                        image: "vllm/vllm-openai:latest".to_string(),
                        ports: vec!["8000:8000".to_string()],
                        environment: std::collections::HashMap::from([
                            ("CUDA_VISIBLE_DEVICES".to_string(), "0,1,2,3".to_string()),
                            ("NCCL_DEBUG".to_string(), "INFO".to_string()),
                        ]),
                    }],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Cheapest, "BARE_METAL"),
                    description: format!("Single-node direct PCIe Gen5 GPU passthrough for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "BARE_METAL_KUBERNETES".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "bm-k8s-control".to_string(),
                            role: "control-plane".to_string(),
                            image: "k8s.gcr.io/kube-apiserver:latest".to_string(),
                            ports: vec!["6443:6443".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "bm-k8s-gpu-worker".to_string(),
                            role: "gpu-worker".to_string(),
                            image: "nvidia/k8s-device-plugin:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Balanced, "BARE_METAL_KUBERNETES"),
                    description: format!("On-premise Bare-Metal Kubernetes cluster with NVIDIA GPU Operator & Calico RoCEv2 for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "BARE_METAL_SLURM_INFINIBAND".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "slurm-controller".to_string(),
                            role: "hpc-scheduler".to_string(),
                            image: "slurm/slurmctld:latest".to_string(),
                            ports: vec!["6817:6817".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "infiniband-subnet-mgr".to_string(),
                            role: "opensm-fabric-manager".to_string(),
                            image: "mellanox/opensm:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::from([
                                ("FABRIC_SPEED".to_string(), "NDR-400Gbps".to_string()),
                                ("RDMA_TRANSPORT".to_string(), "RoCEv2".to_string()),
                            ]),
                        },
                        PlanNode {
                            name: "gpu-compute-rack".to_string(),
                            role: "gpudirect-rdma-cluster".to_string(),
                            image: "nvidia/cuda:12.4.0-devel-ubuntu22.04".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::from([
                                ("NCCL_IB_DISABLE".to_string(), "0".to_string()),
                                ("NCCL_NET_GDR_LEVEL".to_string(), "5".to_string()),
                            ]),
                        },
                    ],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::HighPerformance, "BARE_METAL_SLURM_INFINIBAND"),
                    description: format!("High-performance Slurm HPC cluster with GPUDirect RDMA, InfiniBand fabric, and NCCL ring topology for {}", classification.description),
                },
            ],
            WorkloadKind::StatefulStore => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "BARE_METAL".to_string(),
                    nodes: vec![PlanNode {
                        name: "storage-node-direct".to_string(),
                        role: "direct-attached-nvme".to_string(),
                        image: "postgres:16-alpine".to_string(),
                        ports: vec!["5432:5432".to_string()],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Cheapest, "BARE_METAL"),
                    description: format!("Single-node direct NVMe attached storage for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "BARE_METAL_CEPH".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "ceph-mon-01".to_string(),
                            role: "cluster-monitor".to_string(),
                            image: "ceph/daemon:latest".to_string(),
                            ports: vec!["6789:6789".to_string(), "3300:3300".to_string()],
                            environment: std::collections::HashMap::new(),
                        },
                        PlanNode {
                            name: "ceph-osd-pool".to_string(),
                            role: "distributed-osd-mesh".to_string(),
                            image: "ceph/daemon:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Balanced, "BARE_METAL_CEPH"),
                    description: format!("Distributed Bare-Metal Ceph cluster with dedicated OSD nodes and MON quorum for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "BARE_METAL_NVME_OF".to_string(),
                    nodes: vec![
                        PlanNode {
                            name: "spdk-target-controller".to_string(),
                            role: "nvme-of-controller".to_string(),
                            image: "spdk/spdk:latest".to_string(),
                            ports: vec!["4420:4420".to_string()],
                            environment: std::collections::HashMap::from([
                                ("TRANSPORT".to_string(), "RDMA".to_string()),
                                ("KERNEL_BYPASS".to_string(), "USERS_PACE_UIO".to_string()),
                            ]),
                        },
                        PlanNode {
                            name: "multipath-initiator".to_string(),
                            role: "ana-multipath-io".to_string(),
                            image: "linux/multipath:latest".to_string(),
                            ports: vec![],
                            environment: std::collections::HashMap::new(),
                        },
                    ],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::HighPerformance, "BARE_METAL_NVME_OF"),
                    description: format!("High-performance SPDK NVMe-oF target cluster over RoCEv2 RDMA with zero-copy userspace kernel bypass for {}", classification.description),
                },
            ],
            _ => vec![
                ArchitecturePlan {
                    tier: PlanTier::Cheapest,
                    provider: "BARE_METAL".to_string(),
                    nodes: vec![PlanNode {
                        name: "bm-worker".to_string(),
                        role: "systemd-service".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec![],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Cheapest, "BARE_METAL"),
                    description: format!("Isolated bare-metal systemd execution for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::Balanced,
                    provider: "BARE_METAL_KUBERNETES".to_string(),
                    nodes: vec![PlanNode {
                        name: "bm-k8s-pod".to_string(),
                        role: "host-networking-worker".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec![],
                        environment: std::collections::HashMap::new(),
                    }],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::Balanced, "BARE_METAL_KUBERNETES"),
                    description: format!("Bare-Metal Kubernetes cluster deployment with host networking for {}", classification.description),
                },
                ArchitecturePlan {
                    tier: PlanTier::HighPerformance,
                    provider: "BARE_METAL_NUMA_CORE_PINNING".to_string(),
                    nodes: vec![PlanNode {
                        name: "numa-pinned-engine".to_string(),
                        role: "zero-latency-engine".to_string(),
                        image: "alpine:latest".to_string(),
                        ports: vec![],
                        environment: std::collections::HashMap::from([
                            ("CPU_PINNING".to_string(), "CORES_0-31".to_string()),
                            ("HUGEPAGES_1GB".to_string(), "ENABLED".to_string()),
                            ("INTERRUPT_AFFINITY".to_string(), "ISOLATED".to_string()),
                        ]),
                    }],
                    estimated_monthly_cost_usd: Self::compute_tier_cost(classification, &PlanTier::HighPerformance, "BARE_METAL_NUMA_CORE_PINNING"),
                    description: format!("NUMA-aware bare-metal core pinning with 1GB HugePages pool and isolated hardware interrupt affinity for {}", classification.description),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_pricing_varies_with_scale() {
        // DC-114 classification: 64 nodes, 512 GPUs, 2048 cores, 4TB RAM
        let dc114_class = ProjectClassification {
            workload_kind: WorkloadKind::PythonVllmGpu,
            description: "DC-114 64-node 512-GPU".to_string(),
            recommended_ram_mb: 4_194_304,
            recommended_cpu_cores: 2048,
            recommended_vram_mb: 40_960_000,
            recommended_gpu_count: 512,
        };

        // SM-079 classification: 1 node, 0 GPUs, 2 cores, 2GB RAM
        let sm079_class = ProjectClassification {
            workload_kind: WorkloadKind::PythonVllmGpu,
            description: "SM-079 Python Webhook".to_string(),
            recommended_ram_mb: 2048,
            recommended_cpu_cores: 2,
            recommended_vram_mb: 0,
            recommended_gpu_count: 0,
        };

        let dc114_plans = ArchitecturePlanner::generate_plans(&dc114_class);
        let sm079_plans = ArchitecturePlanner::generate_plans(&sm079_class);

        // Balanced tier cost comparisons
        let dc114_balanced = dc114_plans.iter().find(|p| p.tier == PlanTier::Balanced).unwrap();
        let sm079_balanced = sm079_plans.iter().find(|p| p.tier == PlanTier::Balanced).unwrap();

        // DC-114 has 512 GPUs and 2048 cores: cost must exceed $500,000/mo
        assert!(
            dc114_balanced.estimated_monthly_cost_usd > 500_000.0,
            "DC-114 balanced plan must reflect 512 GPUs and 2048 cores cost, got: {}",
            dc114_balanced.estimated_monthly_cost_usd
        );

        // SM-079 has 0 GPUs and 2 cores: cost must be small (< $200/mo)
        assert!(
            sm079_balanced.estimated_monthly_cost_usd < 200.0,
            "SM-079 balanced plan must be sized for 2 cores and 2GB RAM, got: {}",
            sm079_balanced.estimated_monthly_cost_usd
        );

        // HighPerformance tier must be scaled higher than Balanced
        let dc114_hp = dc114_plans.iter().find(|p| p.tier == PlanTier::HighPerformance).unwrap();
        assert!(dc114_hp.estimated_monthly_cost_usd > dc114_balanced.estimated_monthly_cost_usd);

        // Crucial invariant: two problems in the same workload kind must produce different costs
        assert_ne!(
            dc114_balanced.estimated_monthly_cost_usd,
            sm079_balanced.estimated_monthly_cost_usd,
            "Costs must vary with scale"
        );
    }

    #[test]
    fn test_per_provider_pricing_differences() {
        let class = ProjectClassification {
            workload_kind: WorkloadKind::NodePostgresApi,
            description: "Standard API Service (8 cores, 16GB RAM)".to_string(),
            recommended_ram_mb: 16_384,
            recommended_cpu_cores: 8,
            recommended_vram_mb: 0,
            recommended_gpu_count: 0,
        };

        let aws_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "AWS");
        let gcp_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "GCP");
        let azure_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "AZURE");
        let do_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "DIGITAL_OCEAN");
        let fastly_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "FASTLY_COMPUTE");
        let cf_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "CLOUDFLARE_WORKERS");
        let local_cost = ArchitecturePlanner::compute_tier_cost(&class, &PlanTier::Balanced, "LOCAL_DOCKER");

        // Verified expected figures:
        // AWS: (8*24) + (16*3.50) = 192 + 56 = $248
        // GCP: (8*22.50) + (16*3.10) = 180 + 49.60 = $230
        // Azure: (8*25.50) + (16*3.75) = 204 + 60 = $264
        // DigitalOcean: (8*18) + (16*2.50) = 144 + 40 = $184
        // Fastly: (8*12) + (16*1.80) + 15 = 96 + 28.80 + 15 = $140
        // Cloudflare: (8*10) + (16*1.50) + 10 = 80 + 24 + 10 = $114
        // Local: $0
        assert_eq!(local_cost, 0.0, "Local container cost must be $0");
        assert_eq!(aws_cost, 248.0, "AWS cost must be $248");
        assert_eq!(gcp_cost, 230.0, "GCP cost must be $230");
        assert_eq!(azure_cost, 264.0, "Azure cost must be $264");
        assert_eq!(do_cost, 184.0, "DigitalOcean cost must be $184");
        assert_eq!(fastly_cost, 140.0, "Fastly cost must be $140");
        assert_eq!(cf_cost, 114.0, "Cloudflare cost must be $114");

        // Assert all provider prices are distinct from each other:
        assert_ne!(aws_cost, gcp_cost, "AWS and GCP must have distinct pricing");
        assert_ne!(gcp_cost, azure_cost, "GCP and Azure must have distinct pricing");
        assert_ne!(azure_cost, do_cost, "Azure and DigitalOcean must have distinct pricing");
        assert_ne!(do_cost, fastly_cost, "DigitalOcean and Fastly must have distinct pricing");
        assert_ne!(fastly_cost, cf_cost, "Fastly and Cloudflare must have distinct pricing");
    }

    #[test]
    fn test_bare_metal_system_design_planning() {
        println!("\n=== [TEST] Bare-Metal System Design Planning Verification ===");

        // 1. DC-114 GPU Workload -> BARE_METAL_SLURM_INFINIBAND
        let dc114_gpu_class = ProjectClassification {
            workload_kind: WorkloadKind::PythonVllmGpu,
            description: "DC-114 64-Node Bare-Metal H100 InfiniBand Cluster".to_string(),
            recommended_ram_mb: 4_194_304,
            recommended_cpu_cores: 2048,
            recommended_vram_mb: 40_960_000,
            recommended_gpu_count: 512,
        };

        let bm_gpu_plans = ArchitecturePlanner::generate_bare_metal_plans(&dc114_gpu_class);
        assert_eq!(bm_gpu_plans.len(), 3);
        println!("[+] DC-114 GPU Plans Generated (3 tiers):");
        for p in &bm_gpu_plans {
            println!("    - Tier: {:<16} | Provider: {:<28} | Cost: ${:.2}/mo | Nodes: {}", format!("{:?}", p.tier), p.provider, p.estimated_monthly_cost_usd, p.nodes.len());
        }

        let slurm_hp = bm_gpu_plans.iter().find(|p| p.tier == PlanTier::HighPerformance).unwrap();
        assert_eq!(slurm_hp.provider, "BARE_METAL_SLURM_INFINIBAND");
        assert!(slurm_hp.description.contains("Slurm HPC cluster with GPUDirect RDMA"));
        assert!(slurm_hp.nodes.iter().any(|n| n.role == "gpudirect-rdma-cluster"));
        assert_eq!(slurm_hp.estimated_monthly_cost_usd, 451_189.0, "DC-114 HP cost must be $451,189");
        println!("    [✓] HighPerformance verified: BARE_METAL_SLURM_INFINIBAND (${:.2}/mo)", slurm_hp.estimated_monthly_cost_usd);

        // 2. DC-116 Stateful Storage -> BARE_METAL_CEPH & BARE_METAL_NVME_OF
        let dc116_storage_class = ProjectClassification {
            workload_kind: WorkloadKind::StatefulStore,
            description: "DC-116 Bare-Metal Ceph 2PB NVMe-oF Storage Cluster".to_string(),
            recommended_ram_mb: 65_536,
            recommended_cpu_cores: 32,
            recommended_vram_mb: 0,
            recommended_gpu_count: 0,
        };

        let bm_storage_plans = ArchitecturePlanner::generate_bare_metal_plans(&dc116_storage_class);
        assert_eq!(bm_storage_plans.len(), 3);
        println!("[+] DC-116 Storage Plans Generated (3 tiers):");
        for p in &bm_storage_plans {
            println!("    - Tier: {:<16} | Provider: {:<28} | Cost: ${:.2}/mo | Nodes: {}", format!("{:?}", p.tier), p.provider, p.estimated_monthly_cost_usd, p.nodes.len());
        }

        let ceph_balanced = bm_storage_plans.iter().find(|p| p.tier == PlanTier::Balanced).unwrap();
        assert_eq!(ceph_balanced.provider, "BARE_METAL_CEPH");
        assert!(ceph_balanced.description.contains("Distributed Bare-Metal Ceph cluster"));
        assert!(ceph_balanced.nodes.iter().any(|n| n.role == "distributed-osd-mesh"));
        assert_eq!(ceph_balanced.estimated_monthly_cost_usd, 253.0, "DC-116 Balanced cost must be $253");
        println!("    [✓] Balanced verified: BARE_METAL_CEPH (${:.2}/mo)", ceph_balanced.estimated_monthly_cost_usd);

        let nvme_hp = bm_storage_plans.iter().find(|p| p.tier == PlanTier::HighPerformance).unwrap();
        assert_eq!(nvme_hp.provider, "BARE_METAL_NVME_OF");
        assert!(nvme_hp.description.contains("SPDK NVMe-oF"));
        assert!(nvme_hp.nodes.iter().any(|n| n.role == "nvme-of-controller"));
        assert_eq!(nvme_hp.estimated_monthly_cost_usd, 642.0, "DC-116 HP cost must be $642");
        println!("    [✓] HighPerformance verified: BARE_METAL_NVME_OF (${:.2}/mo)", nvme_hp.estimated_monthly_cost_usd);

        // 3. SC-039 Low-Latency Engine -> BARE_METAL_NUMA_CORE_PINNING
        let sc039_latency_class = ProjectClassification {
            workload_kind: WorkloadKind::RustWasm,
            description: "SC-039 Ultra-Low-Latency Order Matching Engine".to_string(),
            recommended_ram_mb: 32_768,
            recommended_cpu_cores: 16,
            recommended_vram_mb: 0,
            recommended_gpu_count: 0,
        };

        let bm_latency_plans = ArchitecturePlanner::generate_bare_metal_plans(&sc039_latency_class);
        assert_eq!(bm_latency_plans.len(), 3);
        println!("[+] SC-039 Low-Latency Plans Generated (3 tiers):");
        for p in &bm_latency_plans {
            println!("    - Tier: {:<16} | Provider: {:<28} | Cost: ${:.2}/mo | Nodes: {}", format!("{:?}", p.tier), p.provider, p.estimated_monthly_cost_usd, p.nodes.len());
        }

        let numa_hp = bm_latency_plans.iter().find(|p| p.tier == PlanTier::HighPerformance).unwrap();
        assert_eq!(numa_hp.provider, "BARE_METAL_NUMA_CORE_PINNING");
        assert!(numa_hp.description.contains("NUMA-aware bare-metal core pinning"));
        assert!(numa_hp.nodes.iter().any(|n| n.role == "zero-latency-engine"));
        assert_eq!(numa_hp.estimated_monthly_cost_usd, 449.0, "SC-039 HP cost must be $449");
        println!("    [✓] HighPerformance verified: BARE_METAL_NUMA_CORE_PINNING (${:.2}/mo)", numa_hp.estimated_monthly_cost_usd);

        // Scale-tracking invariants across bare-metal workloads:
        assert!(slurm_hp.estimated_monthly_cost_usd > nvme_hp.estimated_monthly_cost_usd * 100.0, "DC-114 GPU cluster must vastly exceed storage cluster");
        assert!(nvme_hp.estimated_monthly_cost_usd > numa_hp.estimated_monthly_cost_usd, "DC-116 32-core storage must exceed SC-039 16-core engine");
        assert_ne!(slurm_hp.estimated_monthly_cost_usd, nvme_hp.estimated_monthly_cost_usd);
        assert_ne!(nvme_hp.estimated_monthly_cost_usd, numa_hp.estimated_monthly_cost_usd);
        println!("=== [END TEST] All 4 Bare-Metal Architectures Dynamically Verified ===\n");
    }
}
