use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sManifestHeader {
    #[serde(rename = "apiVersion")]
    pub api_version: Option<String>,
    pub kind: Option<String>,
    pub metadata: Option<K8sMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sMetadata {
    pub name: Option<String>,
    pub labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sContainerPort {
    #[serde(rename = "containerPort")]
    pub container_port: u16,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sEnvVar {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sResourceRequirements {
    pub requests: Option<HashMap<String, String>>,
    pub limits: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sContainer {
    pub name: String,
    pub image: Option<String>,
    pub ports: Option<Vec<K8sContainerPort>>,
    pub resources: Option<K8sResourceRequirements>,
    pub env: Option<Vec<K8sEnvVar>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sPodSpec {
    pub containers: Vec<K8sContainer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sDeploymentSpec {
    pub replicas: Option<u32>,
    pub template: K8sPodTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sPodTemplate {
    pub spec: K8sPodSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sDeployment {
    pub metadata: Option<K8sMetadata>,
    pub spec: K8sDeploymentSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sPod {
    pub metadata: Option<K8sMetadata>,
    pub spec: K8sPodSpec,
}

pub struct K8sParser;

impl K8sParser {
    pub fn parse_deployment(content: &str) -> Result<K8sDeployment, serde_yaml::Error> {
        if let Ok(dep) = serde_yaml::from_str::<K8sDeployment>(content) {
            return Ok(dep);
        }
        for doc in serde_yaml::Deserializer::from_str(content) {
            if let Ok(val) = serde_yaml::Value::deserialize(doc) {
                if val.get("kind").and_then(|k| k.as_str()) == Some("Deployment") {
                    return serde_yaml::from_value(val);
                }
            }
        }
        serde_yaml::from_str(content)
    }

    pub fn parse_pod(content: &str) -> Result<K8sPod, serde_yaml::Error> {
        if let Ok(pod) = serde_yaml::from_str::<K8sPod>(content) {
            return Ok(pod);
        }
        for doc in serde_yaml::Deserializer::from_str(content) {
            if let Ok(val) = serde_yaml::Value::deserialize(doc) {
                if val.get("kind").and_then(|k| k.as_str()) == Some("Pod") {
                    return serde_yaml::from_value(val);
                }
            }
        }
        serde_yaml::from_str(content)
    }

    /// Parses all documents in a multi-document YAML stream
    pub fn parse_multi_docs(content: &str) -> Vec<serde_yaml::Value> {
        let mut results = Vec::new();
        for doc in serde_yaml::Deserializer::from_str(content) {
            if let Ok(val) = serde_yaml::Value::deserialize(doc) {
                results.push(val);
            }
        }
        results
    }

    /// Converts K8s CPU string (e.g. "500m", "1", "2000m") to CPU cores integer
    pub fn parse_cpu_to_cores(cpu_str: &str) -> u32 {
        let clean = cpu_str.trim().to_lowercase();
        if clean.ends_with('m') {
            let millis: f64 = clean.trim_end_matches('m').parse().unwrap_or(1000.0);
            (millis / 1000.0).ceil().max(1.0) as u32
        } else {
            let val: f64 = clean.parse().unwrap_or(1.0);
            val.ceil().max(1.0) as u32
        }
    }

    /// Converts K8s Memory string (e.g. "512Mi", "1Gi", "2048M") to RAM MB integer
    pub fn parse_memory_to_mb(mem_str: &str) -> u32 {
        let clean = mem_str.trim().to_uppercase();
        if clean.ends_with("GI") || clean.ends_with("G") {
            let num: f64 = clean
                .trim_matches(|c: char| !c.is_numeric() && c != '.')
                .parse()
                .unwrap_or(1.0);
            (num * 1024.0) as u32
        } else if clean.ends_with("MI") || clean.ends_with("M") {
            let num: f64 = clean
                .trim_matches(|c: char| !c.is_numeric() && c != '.')
                .parse()
                .unwrap_or(512.0);
            num as u32
        } else if clean.ends_with("KI") || clean.ends_with("K") {
            1
        } else {
            clean.parse().unwrap_or(512)
        }
    }
}
