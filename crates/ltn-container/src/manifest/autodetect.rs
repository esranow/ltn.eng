use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestKind {
    DockerCompose,
    KubernetesDeployment,
    KubernetesPod,
    KubernetesService,
    K6sManifest,
    LtnNativeYaml,
    Unknown,
}

pub struct ManifestDetector;

impl ManifestDetector {
    /// Detect the manifest format from a YAML/JSON string
    pub fn detect_str(content: &str) -> ManifestKind {
        for doc in serde_yaml::Deserializer::from_str(content) {
            if let Ok(val) = serde_yaml::Value::deserialize(doc) {
                // Check Docker Compose
                if val.get("services").is_some() {
                    return ManifestKind::DockerCompose;
                }

                // Check Kubernetes / K6s specs
                if let Some(kind) = val.get("kind").and_then(|k| k.as_str()) {
                    let api_ver = val.get("apiVersion").and_then(|a| a.as_str()).unwrap_or("");
                    if api_ver.contains("k6.io") || kind.eq_ignore_ascii_case("K6") {
                        return ManifestKind::K6sManifest;
                    }
                    match kind {
                        "Deployment" => return ManifestKind::KubernetesDeployment,
                        "Pod" => return ManifestKind::KubernetesPod,
                        "Service" => return ManifestKind::KubernetesService,
                        _ => {}
                    }
                }

                // Check LTN native YAML
                if val.get("workloads").is_some() || val.get("architecture").is_some() {
                    return ManifestKind::LtnNativeYaml;
                }
            }
        }

        // Check if multi-document YAML or unparsed string contains Deployment or Pod
        if content.contains("kind: Deployment") {
            return ManifestKind::KubernetesDeployment;
        }
        if content.contains("kind: Pod") {
            return ManifestKind::KubernetesPod;
        }
        if content.contains("kind: Service") {
            return ManifestKind::KubernetesService;
        }

        ManifestKind::Unknown
    }

    /// Detect manifest format from a file path
    pub fn detect_file(path: &std::path::Path) -> std::io::Result<ManifestKind> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::detect_str(&content))
    }
}
