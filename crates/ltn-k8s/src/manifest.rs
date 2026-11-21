use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sAppConfig {
    pub name: String,
    pub namespace: String,
    pub image: String,
    pub replicas: i32,
    pub container_port: u16,
    pub service_port: u16,
    pub cpu_request: String,
    pub memory_request: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub health_path: String,
    pub env_vars: HashMap<String, String>,
    pub secret_keys: Vec<String>,
}

impl Default for K8sAppConfig {
    fn default() -> Self {
        Self {
            name: "app".to_string(),
            namespace: "default".to_string(),
            image: "nginx:latest".to_string(),
            replicas: 2,
            container_port: 80,
            service_port: 80,
            cpu_request: "250m".to_string(),
            memory_request: "512Mi".to_string(),
            cpu_limit: "1000m".to_string(),
            memory_limit: "1024Mi".to_string(),
            health_path: "/".to_string(),
            env_vars: HashMap::new(),
            secret_keys: Vec::new(),
        }
    }
}

pub struct K8sManifestSynthesizer;

impl K8sManifestSynthesizer {
    pub fn generate_deployment(cfg: &K8sAppConfig) -> serde_json::Value {
        let env_list: Vec<serde_json::Value> = cfg
            .env_vars
            .iter()
            .map(|(k, v)| serde_json::json!({ "name": k, "value": v }))
            .collect();

        serde_json::json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {
                "name": cfg.name,
                "namespace": cfg.namespace,
                "labels": { "app": cfg.name, "managed-by": "ltn-eng" }
            },
            "spec": {
                "replicas": cfg.replicas,
                "selector": {
                    "matchLabels": { "app": cfg.name }
                },
                "template": {
                    "metadata": {
                        "labels": { "app": cfg.name }
                    },
                    "spec": {
                        "containers": [{
                            "name": cfg.name,
                            "image": cfg.image,
                            "ports": [{ "containerPort": cfg.container_port }],
                            "resources": {
                                "requests": {
                                    "cpu": cfg.cpu_request,
                                    "memory": cfg.memory_request
                                },
                                "limits": {
                                    "cpu": cfg.cpu_limit,
                                    "memory": cfg.memory_limit
                                }
                            },
                            "readinessProbe": {
                                "httpGet": {
                                    "path": cfg.health_path,
                                    "port": cfg.container_port
                                },
                                "initialDelaySeconds": 5,
                                "periodSeconds": 10
                            },
                            "livenessProbe": {
                                "httpGet": {
                                    "path": cfg.health_path,
                                    "port": cfg.container_port
                                },
                                "initialDelaySeconds": 15,
                                "periodSeconds": 20
                            },
                            "env": env_list
                        }]
                    }
                }
            }
        })
    }

    pub fn generate_service(cfg: &K8sAppConfig) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": format!("{}-svc", cfg.name),
                "namespace": cfg.namespace,
                "labels": { "app": cfg.name, "managed-by": "ltn-eng" }
            },
            "spec": {
                "selector": { "app": cfg.name },
                "ports": [{
                    "name": "http",
                    "port": cfg.service_port,
                    "targetPort": cfg.container_port
                }],
                "type": "ClusterIP"
            }
        })
    }

    pub fn generate_ingress(cfg: &K8sAppConfig, host: &str) -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "networking.k8s.io/v1",
            "kind": "Ingress",
            "metadata": {
                "name": format!("{}-ingress", cfg.name),
                "namespace": cfg.namespace,
                "annotations": {
                    "cert-manager.io/cluster-issuer": "letsencrypt-prod",
                    "kubernetes.io/ingress.class": "nginx"
                }
            },
            "spec": {
                "tls": [{
                    "hosts": [host],
                    "secretName": format!("{}-tls", cfg.name)
                }],
                "rules": [{
                    "host": host,
                    "http": {
                        "paths": [{
                            "path": "/",
                            "pathType": "Prefix",
                            "backend": {
                                "service": {
                                    "name": format!("{}-svc", cfg.name),
                                    "port": { "number": cfg.service_port }
                                }
                            }
                        }]
                    }
                }]
            }
        })
    }

    pub fn to_yaml(manifest: &serde_json::Value) -> Result<String> {
        let yaml = serde_yaml::to_string(manifest)?;
        Ok(yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_manifest_synthesis() {
        let mut cfg = K8sAppConfig::default();
        cfg.name = "inference-api".to_string();
        cfg.namespace = "production".to_string();
        cfg.container_port = 8000;
        cfg.service_port = 80;
        cfg.env_vars.insert("MODEL_NAME".to_string(), "rippl-heat".to_string());

        let dep = K8sManifestSynthesizer::generate_deployment(&cfg);
        assert_eq!(dep["metadata"]["name"], "inference-api");
        assert_eq!(dep["metadata"]["namespace"], "production");
        assert_eq!(dep["spec"]["replicas"], 2);

        let svc = K8sManifestSynthesizer::generate_service(&cfg);
        assert_eq!(svc["metadata"]["name"], "inference-api-svc");
        assert_eq!(svc["spec"]["ports"][0]["port"], 80);

        let ingress = K8sManifestSynthesizer::generate_ingress(&cfg, "api.rippl.ai");
        assert_eq!(ingress["spec"]["rules"][0]["host"], "api.rippl.ai");

        let yaml = K8sManifestSynthesizer::to_yaml(&dep).unwrap();
        assert!(yaml.contains("kind: Deployment"));
        assert!(yaml.contains("name: inference-api"));
    }
}
