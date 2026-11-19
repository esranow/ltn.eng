use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeSpec {
    pub version: Option<String>,
    pub services: HashMap<String, ComposeService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeService {
    pub image: Option<String>,
    pub ports: Option<Vec<String>>,
    pub environment: Option<serde_yaml::Value>,
    pub volumes: Option<Vec<String>>,
    pub depends_on: Option<serde_yaml::Value>,
    pub restart: Option<String>,
    pub deploy: Option<ComposeDeploy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeDeploy {
    pub replicas: Option<u32>,
    pub resources: Option<ComposeResources>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeResources {
    pub limits: Option<ComposeResourceLimits>,
    pub reservations: Option<ComposeResourceLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeResourceLimits {
    pub cpus: Option<String>,
    pub memory: Option<String>,
}

pub struct ComposeParser;

impl ComposeParser {
    pub fn parse(content: &str) -> Result<ComposeSpec, serde_yaml::Error> {
        serde_yaml::from_str(content)
    }

    pub fn parse_file(path: &std::path::Path) -> Result<ComposeSpec, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let spec = Self::parse(&content)?;
        Ok(spec)
    }

    /// Helper to parse memory strings like "1024M", "2G", "512MiB" to MB
    pub fn parse_memory_mb(mem_str: &str) -> u32 {
        let clean = mem_str.trim().to_uppercase();
        if clean.ends_with("G")
            || clean.ends_with("GB")
            || clean.ends_with("GI")
            || clean.ends_with("GIB")
        {
            let num: f64 = clean
                .trim_matches(|c: char| !c.is_numeric() && c != '.')
                .parse()
                .unwrap_or(1.0);
            (num * 1024.0) as u32
        } else if clean.ends_with("M")
            || clean.ends_with("MB")
            || clean.ends_with("MI")
            || clean.ends_with("MIB")
        {
            let num: f64 = clean
                .trim_matches(|c: char| !c.is_numeric() && c != '.')
                .parse()
                .unwrap_or(512.0);
            num as u32
        } else if clean.ends_with("K") || clean.ends_with("KB") {
            1
        } else {
            clean.parse().unwrap_or(512)
        }
    }

    /// Helper to parse CPU strings like "0.5", "1", "2.0" to integer cores (rounded up to at least 1)
    pub fn parse_cpu_cores(cpu_str: &str) -> u32 {
        let val: f64 = cpu_str.trim().parse().unwrap_or(1.0);
        val.ceil().max(1.0) as u32
    }
}
