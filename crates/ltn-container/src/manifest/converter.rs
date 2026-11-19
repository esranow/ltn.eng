use super::autodetect::{ManifestDetector, ManifestKind};
use super::compose_parser::{ComposeParser, ComposeSpec};
use super::k8s_parser::{K8sDeployment, K8sParser, K8sPod};
use ltn_core::ir::{
    Architecture, ComputeRequirements, NetworkRequirements, PlacementConstraints, PortDefinition,
    ProviderTarget, ReplicaPolicy, StorageRequirements, WorkloadDefinition, WorkloadKind,
};
use uuid::Uuid;

pub struct ManifestConverter;

impl ManifestConverter {
    /// Ingests any manifest string (Compose, K8s, K6s, YAML) and returns a typed Architecture IR
    pub fn convert_str(content: &str, project_name: &str) -> anyhow::Result<Architecture> {
        let kind = ManifestDetector::detect_str(content);
        match kind {
            ManifestKind::DockerCompose => {
                let spec = ComposeParser::parse(content)?;
                Ok(Self::from_compose(&spec, project_name))
            }
            ManifestKind::KubernetesDeployment => {
                let deployment = K8sParser::parse_deployment(content)?;
                Ok(Self::from_k8s_deployment(&deployment, project_name))
            }
            ManifestKind::KubernetesPod => {
                let pod = K8sParser::parse_pod(content)?;
                Ok(Self::from_k8s_pod(&pod, project_name))
            }
            ManifestKind::K6sManifest => {
                // K6s / performance test manifest converted to worker workload
                let mut arch =
                    Architecture::new(project_name, ProviderTarget::DeterministicContainer);
                arch.workloads.push(WorkloadDefinition {
                    id: Uuid::new_v4(),
                    name: format!("{}-k6s-runner", project_name),
                    kind: WorkloadKind::GoWorker,
                    container_image: "grafana/k6:latest".to_string(),
                    entrypoint: Some("k6 run /scripts/test.js".to_string()),
                    ports: vec![PortDefinition {
                        container_port: 6565,
                        host_port: Some(6565),
                        protocol: "TCP".to_string(),
                    }],
                    secrets: Vec::new(),
                    compute: ComputeRequirements {
                        cpu_cores: 2,
                        ram_mb: 2048,
                        gpu_count: 0,
                        vram_mb: 0,
                        gpu_model_preference: None,
                    },
                    storage: StorageRequirements::default(),
                    network: NetworkRequirements {
                        ingress_public: false,
                        required_bandwidth_gbps: 1,
                        service_mesh_wireguard: false,
                        allowed_cidr_blocks: vec!["127.0.0.1/32".to_string()],
                    },
                    replicas: ReplicaPolicy::default(),
                    placement: PlacementConstraints::default(),
                });
                Ok(arch)
            }
            _ => {
                // Fallback / Native YAML or generic container
                let mut arch =
                    Architecture::new(project_name, ProviderTarget::DeterministicContainer);
                arch.workloads.push(WorkloadDefinition {
                    id: Uuid::new_v4(),
                    name: format!("{}-service", project_name),
                    kind: WorkloadKind::GenericContainer,
                    container_image: "alpine:latest".to_string(),
                    entrypoint: None,
                    ports: vec![PortDefinition {
                        container_port: 8080,
                        host_port: Some(8080),
                        protocol: "TCP".to_string(),
                    }],
                    secrets: Vec::new(),
                    compute: ComputeRequirements {
                        cpu_cores: 1,
                        ram_mb: 1024,
                        gpu_count: 0,
                        vram_mb: 0,
                        gpu_model_preference: None,
                    },
                    storage: StorageRequirements::default(),
                    network: NetworkRequirements {
                        ingress_public: true,
                        required_bandwidth_gbps: 1,
                        service_mesh_wireguard: false,
                        allowed_cidr_blocks: vec!["0.0.0.0/0".to_string()],
                    },
                    replicas: ReplicaPolicy::default(),
                    placement: PlacementConstraints::default(),
                });
                Ok(arch)
            }
        }
    }

    /// Converts a Docker Compose spec into Architecture IR
    pub fn from_compose(spec: &ComposeSpec, project_name: &str) -> Architecture {
        let mut arch = Architecture::new(project_name, ProviderTarget::DeterministicContainer);
        arch.metadata.description = "Converted from Docker Compose manifest".to_string();

        for (svc_name, svc) in &spec.services {
            let mut ports = Vec::new();
            if let Some(port_list) = &svc.ports {
                for p in port_list {
                    let parts: Vec<&str> = p.split(':').collect();
                    if parts.len() == 2 {
                        let host: u16 = parts[0].parse().unwrap_or(8080);
                        let container: u16 = parts[1].parse().unwrap_or(host);
                        ports.push(PortDefinition {
                            container_port: container,
                            host_port: Some(host),
                            protocol: "TCP".to_string(),
                        });
                    } else if parts.len() == 1 {
                        let port_num: u16 = parts[0].parse().unwrap_or(8080);
                        ports.push(PortDefinition {
                            container_port: port_num,
                            host_port: Some(port_num),
                            protocol: "TCP".to_string(),
                        });
                    }
                }
            }

            // Extract resource limits if specified in deploy.resources
            let (cpu_cores, ram_mb) = if let Some(deploy) = &svc.deploy {
                if let Some(res) = &deploy.resources {
                    if let Some(limits) = &res.limits {
                        let c = limits
                            .cpus
                            .as_deref()
                            .map(ComposeParser::parse_cpu_cores)
                            .unwrap_or(1);
                        let m = limits
                            .memory
                            .as_deref()
                            .map(ComposeParser::parse_memory_mb)
                            .unwrap_or(1024);
                        (c, m)
                    } else {
                        (1, 1024)
                    }
                } else {
                    (1, 1024)
                }
            } else {
                (1, 1024)
            };

            let kind = if svc_name.contains("db")
                || svc_name.contains("postgres")
                || svc_name.contains("redis")
            {
                WorkloadKind::StatefulStore
            } else if svc_name.contains("api") || svc_name.contains("backend") {
                WorkloadKind::NodePostgresApi
            } else if svc_name.contains("web") || svc_name.contains("frontend") {
                WorkloadKind::StaticWeb
            } else {
                WorkloadKind::GenericContainer
            };

            let workload = WorkloadDefinition {
                id: Uuid::new_v4(),
                name: svc_name.clone(),
                kind,
                container_image: svc
                    .image
                    .clone()
                    .unwrap_or_else(|| format!("{}:latest", svc_name)),
                entrypoint: None,
                ports,
                secrets: Vec::new(),
                compute: ComputeRequirements {
                    cpu_cores,
                    ram_mb,
                    gpu_count: 0,
                    vram_mb: 0,
                    gpu_model_preference: None,
                },
                storage: StorageRequirements::default(),
                network: NetworkRequirements {
                    ingress_public: true,
                    required_bandwidth_gbps: 1,
                    service_mesh_wireguard: false,
                    allowed_cidr_blocks: vec!["0.0.0.0/0".to_string()],
                },
                replicas: ReplicaPolicy {
                    desired_replicas: svc.deploy.as_ref().and_then(|d| d.replicas).unwrap_or(1),
                    min_replicas: 1,
                    max_replicas: 5,
                },
                placement: PlacementConstraints::default(),
            };

            arch.workloads.push(workload);
        }

        arch
    }

    /// Converts a Kubernetes Deployment into Architecture IR
    pub fn from_k8s_deployment(deployment: &K8sDeployment, project_name: &str) -> Architecture {
        let mut arch = Architecture::new(project_name, ProviderTarget::DeterministicContainer);
        arch.metadata.description = "Converted from Kubernetes Deployment manifest".to_string();

        let desired_replicas = deployment.spec.replicas.unwrap_or(1);

        for container in &deployment.spec.template.spec.containers {
            let mut ports = Vec::new();
            if let Some(port_defs) = &container.ports {
                for p in port_defs {
                    ports.push(PortDefinition {
                        container_port: p.container_port,
                        host_port: Some(p.container_port),
                        protocol: p.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                    });
                }
            }

            let mut cpu_cores = 1;
            let mut ram_mb = 1024;

            if let Some(res) = &container.resources {
                if let Some(limits) = &res.limits {
                    if let Some(cpu_str) = limits.get("cpu") {
                        cpu_cores = K8sParser::parse_cpu_to_cores(cpu_str);
                    }
                    if let Some(mem_str) = limits.get("memory") {
                        ram_mb = K8sParser::parse_memory_to_mb(mem_str);
                    }
                } else if let Some(reqs) = &res.requests {
                    if let Some(cpu_str) = reqs.get("cpu") {
                        cpu_cores = K8sParser::parse_cpu_to_cores(cpu_str);
                    }
                    if let Some(mem_str) = reqs.get("memory") {
                        ram_mb = K8sParser::parse_memory_to_mb(mem_str);
                    }
                }
            }

            let kind = if container.name.contains("gpu") || container.name.contains("vllm") {
                WorkloadKind::PythonVllmGpu
            } else if container.name.contains("api") {
                WorkloadKind::NodePostgresApi
            } else if container.name.contains("worker") {
                WorkloadKind::GoWorker
            } else {
                WorkloadKind::GenericContainer
            };

            let workload = WorkloadDefinition {
                id: Uuid::new_v4(),
                name: container.name.clone(),
                kind,
                container_image: container
                    .image
                    .clone()
                    .unwrap_or_else(|| "alpine:latest".to_string()),
                entrypoint: None,
                ports,
                secrets: Vec::new(),
                compute: ComputeRequirements {
                    cpu_cores,
                    ram_mb,
                    gpu_count: 0,
                    vram_mb: 0,
                    gpu_model_preference: None,
                },
                storage: StorageRequirements::default(),
                network: NetworkRequirements {
                    ingress_public: true,
                    required_bandwidth_gbps: 1,
                    service_mesh_wireguard: false,
                    allowed_cidr_blocks: vec!["0.0.0.0/0".to_string()],
                },
                replicas: ReplicaPolicy {
                    desired_replicas,
                    min_replicas: 1,
                    max_replicas: desired_replicas.max(3),
                },
                placement: PlacementConstraints::default(),
            };

            arch.workloads.push(workload);
        }

        arch
    }

    /// Converts a Kubernetes Pod into Architecture IR
    pub fn from_k8s_pod(pod: &K8sPod, project_name: &str) -> Architecture {
        let mut arch = Architecture::new(project_name, ProviderTarget::DeterministicContainer);
        arch.metadata.description = "Converted from Kubernetes Pod manifest".to_string();

        for container in &pod.spec.containers {
            let mut ports = Vec::new();
            if let Some(port_defs) = &container.ports {
                for p in port_defs {
                    ports.push(PortDefinition {
                        container_port: p.container_port,
                        host_port: Some(p.container_port),
                        protocol: p.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                    });
                }
            }

            let workload = WorkloadDefinition {
                id: Uuid::new_v4(),
                name: container.name.clone(),
                kind: WorkloadKind::GenericContainer,
                container_image: container
                    .image
                    .clone()
                    .unwrap_or_else(|| "alpine:latest".to_string()),
                entrypoint: None,
                ports,
                secrets: Vec::new(),
                compute: ComputeRequirements {
                    cpu_cores: 1,
                    ram_mb: 512,
                    gpu_count: 0,
                    vram_mb: 0,
                    gpu_model_preference: None,
                },
                storage: StorageRequirements::default(),
                network: NetworkRequirements {
                    ingress_public: true,
                    required_bandwidth_gbps: 1,
                    service_mesh_wireguard: false,
                    allowed_cidr_blocks: vec!["0.0.0.0/0".to_string()],
                },
                replicas: ReplicaPolicy::default(),
                placement: PlacementConstraints::default(),
            };

            arch.workloads.push(workload);
        }

        arch
    }
}
