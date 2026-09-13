use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StructuredScaleProfile {
    pub node_count: Option<u32>,
    pub gpu_count: Option<u32>,
    pub gpu_model: Option<String>,
    pub cpu_cores: Option<u32>,
    pub memory_mb: Option<u32>,
    pub vram_mb: Option<u32>,
    pub workload_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ManifestDiscovery {
    pub has_package_json: bool,
    pub has_cargo_toml: bool,
    pub has_pyproject_toml: bool,
    pub has_go_mod: bool,
    pub has_pom_xml: bool,
    pub has_dockerfile: bool,
    pub detected_files: Vec<String>,
    pub structured_profile: Option<StructuredScaleProfile>,
}

impl ManifestDiscovery {
    pub fn has_docker_compose(&self) -> bool {
        self.detected_files.iter().any(|f| f == "docker-compose.yml" || f == "docker-compose.yaml" || f == "compose.yml" || f == "compose.yaml")
    }

    pub fn is_empty(&self) -> bool {
        self.detected_files.is_empty()
    }
}

pub struct ManifestParser;

impl ManifestParser {
    pub fn parse_project_profile(path: &Path) -> Option<StructuredScaleProfile> {
        let content = std::fs::read_to_string(path).ok()?;
        let val: serde_json::Value = serde_json::from_str(&content).ok()?;

        let mut profile = StructuredScaleProfile::default();
        if let Some(wt) = val.get("workload_type").and_then(|v| v.as_str()) {
            profile.workload_type = Some(wt.to_string());
        }

        let demands = val.get("resource_demands").unwrap_or(&val);
        if let Some(nc) = demands.get("node_count").and_then(|v| v.as_u64()) {
            profile.node_count = Some(nc as u32);
        }
        if let Some(gc) = demands.get("gpu_count").and_then(|v| v.as_u64()) {
            profile.gpu_count = Some(gc as u32);
        }
        if let Some(gm) = demands.get("gpu_model").and_then(|v| v.as_str()) {
            profile.gpu_model = Some(gm.to_string());
        }
        if let Some(cc) = demands.get("cpu_cores").and_then(|v| v.as_u64()) {
            profile.cpu_cores = Some(cc as u32);
        }
        if let Some(mm) = demands.get("memory_mb").and_then(|v| v.as_u64()) {
            profile.memory_mb = Some(mm as u32);
        }
        if let Some(vm) = demands.get("vram_mb").and_then(|v| v.as_u64()) {
            profile.vram_mb = Some(vm as u32);
        }

        Some(profile)
    }

    pub fn parse_topology_json(path: &Path) -> Option<StructuredScaleProfile> {
        let content = std::fs::read_to_string(path).ok()?;
        let val: serde_json::Value = serde_json::from_str(&content).ok()?;
        let mut profile = StructuredScaleProfile::default();

        if let Some(nodes) = val.get("nodes").and_then(|v| v.as_array()) {
            profile.node_count = Some(nodes.len() as u32);
            let mut total_gpus: u32 = 0;
            let mut total_cores: u32 = 0;
            let mut total_ram: u32 = 0;
            let mut total_vram: u32 = 0;
            let mut model_found: Option<String> = None;

            for n in nodes {
                if let Some(gc) = n.get("gpu_count").and_then(|v| v.as_u64()) {
                    total_gpus += gc as u32;
                }
                if let Some(cc) = n.get("cpu_cores").and_then(|v| v.as_u64()) {
                    total_cores += cc as u32;
                }
                if let Some(rm) = n.get("memory_mb").and_then(|v| v.as_u64()) {
                    total_ram += rm as u32;
                }
                if let Some(vm) = n.get("vram_mb").and_then(|v| v.as_u64()) {
                    total_vram += vm as u32;
                }
                if model_found.is_none() {
                    if let Some(gm) = n.get("gpu_model").and_then(|v| v.as_str()) {
                        model_found = Some(gm.to_string());
                    }
                }
            }
            if total_gpus > 0 { profile.gpu_count = Some(total_gpus); }
            if total_cores > 0 { profile.cpu_cores = Some(total_cores); }
            if total_ram > 0 { profile.memory_mb = Some(total_ram); }
            if total_vram > 0 { profile.vram_mb = Some(total_vram); }
            profile.gpu_model = model_found;
        } else {
            if let Some(nc) = val.get("node_count").and_then(|v| v.as_u64()) {
                profile.node_count = Some(nc as u32);
            }
            if let Some(gc) = val.get("gpu_count").and_then(|v| v.as_u64()) {
                profile.gpu_count = Some(gc as u32);
            }
            if let Some(gm) = val.get("gpu_model").and_then(|v| v.as_str()) {
                profile.gpu_model = Some(gm.to_string());
            }
            if let Some(cc) = val.get("cpu_cores").and_then(|v| v.as_u64()) {
                profile.cpu_cores = Some(cc as u32);
            }
            if let Some(mm) = val.get("memory_mb").and_then(|v| v.as_u64()) {
                profile.memory_mb = Some(mm as u32);
            }
            if let Some(vm) = val.get("vram_mb").and_then(|v| v.as_u64()) {
                profile.vram_mb = Some(vm as u32);
            }
        }
        Some(profile)
    }

    pub fn parse_pyproject_ltn_tool(path: &Path) -> Option<StructuredScaleProfile> {
        let content = std::fs::read_to_string(path).ok()?;
        let mut in_ltn_section = false;
        let mut profile = StructuredScaleProfile::default();
        let mut found_any = false;

        for line in content.lines() {
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') {
                let sec = t.trim_start_matches('[').trim_end_matches(']');
                in_ltn_section = sec.starts_with("tool.ltn");
                continue;
            }
            if in_ltn_section && t.contains('=') {
                let parts: Vec<&str> = t.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let k = parts[0].trim();
                    let v = parts[1].trim().trim_matches('"').trim_matches('\'');
                    match k {
                        "node_count" => if let Ok(n) = v.parse::<u32>() { profile.node_count = Some(n); found_any = true; },
                        "gpu_count" => if let Ok(n) = v.parse::<u32>() { profile.gpu_count = Some(n); found_any = true; },
                        "gpu_model" => { profile.gpu_model = Some(v.to_string()); found_any = true; },
                        "cpu_cores" => if let Ok(n) = v.parse::<u32>() { profile.cpu_cores = Some(n); found_any = true; },
                        "memory_mb" => if let Ok(n) = v.parse::<u32>() { profile.memory_mb = Some(n); found_any = true; },
                        "vram_mb" => if let Ok(n) = v.parse::<u32>() { profile.vram_mb = Some(n); found_any = true; },
                        "workload_type" => { profile.workload_type = Some(v.to_string()); found_any = true; },
                        _ => {}
                    }
                }
            }
        }

        if found_any {
            Some(profile)
        } else {
            None
        }
    }

    pub fn discover_in_dir(path: &Path) -> ManifestDiscovery {
        let mut detected_files = Vec::new();
        let mut discovery = ManifestDiscovery {
            has_package_json: false,
            has_cargo_toml: false,
            has_pyproject_toml: false,
            has_go_mod: false,
            has_pom_xml: false,
            has_dockerfile: false,
            detected_files: Vec::new(),
            structured_profile: None,
        };

        if !path.exists() || !path.is_dir() {
            return discovery;
        }

        // 1. Check for schema-driven project_profile.json
        let proj_profile_path = path.join("project_profile.json");
        if proj_profile_path.exists() {
            if let Some(prof) = Self::parse_project_profile(&proj_profile_path) {
                discovery.structured_profile = Some(prof);
                detected_files.push("project_profile.json".to_string());
            }
        }

        // 2. Check for topology.json
        let topo_path = path.join("topology.json");
        if topo_path.exists() {
            if let Some(prof) = Self::parse_topology_json(&topo_path) {
                if discovery.structured_profile.is_none() {
                    discovery.structured_profile = Some(prof);
                }
                detected_files.push("topology.json".to_string());
            }
        }

        let pkg_path = path.join("package.json");
        if pkg_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&pkg_path) {
                if let Ok(pkg_val) = serde_json::from_str::<serde_json::Value>(&content) {
                    discovery.has_package_json = true;
                    detected_files.push("package.json".to_string());
                    if discovery.structured_profile.is_none() {
                        if let Some(ltn_obj) = pkg_val.get("ltn") {
                            let mut prof = StructuredScaleProfile::default();
                            let mut found = false;
                            if let Some(nc) = ltn_obj.get("node_count").and_then(|v| v.as_u64()) {
                                prof.node_count = Some(nc as u32);
                                found = true;
                            }
                            if let Some(cc) = ltn_obj.get("cpu_cores").and_then(|v| v.as_u64()) {
                                prof.cpu_cores = Some(cc as u32);
                                found = true;
                            }
                            if let Some(mm) = ltn_obj.get("memory_mb").and_then(|v| v.as_u64()) {
                                prof.memory_mb = Some(mm as u32);
                                found = true;
                            }
                            if let Some(gc) = ltn_obj.get("gpu_count").and_then(|v| v.as_u64()) {
                                prof.gpu_count = Some(gc as u32);
                                found = true;
                            }
                            if found {
                                discovery.structured_profile = Some(prof);
                            }
                        }
                    }
                }
            }
        }

        let pyproject_path = path.join("pyproject.toml");
        if pyproject_path.exists() {
            discovery.has_pyproject_toml = true;
            detected_files.push("pyproject.toml".to_string());
            if discovery.structured_profile.is_none() {
                if let Some(prof) = Self::parse_pyproject_ltn_tool(&pyproject_path) {
                    discovery.structured_profile = Some(prof);
                }
            }
        } else if path.join("requirements.txt").exists() {
            discovery.has_pyproject_toml = true;
            detected_files.push("requirements.txt".to_string());
        }

        if path.join("Cargo.toml").exists() {
            discovery.has_cargo_toml = true;
            detected_files.push("Cargo.toml".to_string());
        }

        if path.join("go.mod").exists() {
            discovery.has_go_mod = true;
            detected_files.push("go.mod".to_string());
        }

        if path.join("pom.xml").exists() {
            discovery.has_pom_xml = true;
            detected_files.push("pom.xml".to_string());
        }

        if path.join("Dockerfile").exists() {
            discovery.has_dockerfile = true;
            detected_files.push("Dockerfile".to_string());
        }

        if path.join("docker-compose.yml").exists()
            || path.join("docker-compose.yaml").exists()
            || path.join("compose.yml").exists()
            || path.join("compose.yaml").exists()
        {
            detected_files.push("docker-compose.yml".to_string());
        }

        discovery.detected_files = detected_files;
        discovery
    }

    pub fn parse_package_json(path: &Path) -> Result<serde_json::Value, String> {
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("package.json")
        };
        if !target.exists() {
            return Err(format!("File not found: {}", target.display()));
        }
        let content = std::fs::read_to_string(&target)
            .map_err(|e| format!("Failed to read {}: {}", target.display(), e))?;
        serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("Corrupt JSON in {}: {}", target.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_profile_json() {
        let json = r#"{
            "project_id": "DC-114",
            "project_path": ".",
            "workload_type": "PYTHON_VLLM_GPU",
            "resource_demands": {
                "node_count": 32,
                "gpu_count": 256,
                "gpu_model": "H100-SXM5-80GB",
                "cpu_cores": 1024,
                "memory_mb": 2097152,
                "vram_mb": 20480000,
                "storage_gb": 10000
            }
        }"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_profile");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("project_profile.json");
        std::fs::write(&file_path, json).unwrap();

        let prof = ManifestParser::parse_project_profile(&file_path).expect("Must parse project_profile.json");
        assert_eq!(prof.node_count, Some(32));
        assert_eq!(prof.gpu_count, Some(256));
        assert_eq!(prof.cpu_cores, Some(1024));
        assert_eq!(prof.memory_mb, Some(2097152));
        assert_eq!(prof.vram_mb, Some(20480000));
        assert_eq!(prof.gpu_model.as_deref(), Some("H100-SXM5-80GB"));
        assert_eq!(prof.workload_type.as_deref(), Some("PYTHON_VLLM_GPU"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_parse_topology_json_nodes_array() {
        let json = r#"{
            "nodes": [
                { "name": "node-1", "cpu_cores": 32, "memory_mb": 65536, "gpu_count": 8, "vram_mb": 640000, "gpu_model": "H100" },
                { "name": "node-2", "cpu_cores": 32, "memory_mb": 65536, "gpu_count": 8, "vram_mb": 640000, "gpu_model": "H100" }
            ]
        }"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_topology");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("topology.json");
        std::fs::write(&file_path, json).unwrap();

        let prof = ManifestParser::parse_topology_json(&file_path).expect("Must parse topology.json");
        assert_eq!(prof.node_count, Some(2));
        assert_eq!(prof.gpu_count, Some(16));
        assert_eq!(prof.cpu_cores, Some(64));
        assert_eq!(prof.memory_mb, Some(131072));
        assert_eq!(prof.vram_mb, Some(1280000));
        assert_eq!(prof.gpu_model.as_deref(), Some("H100"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_parse_pyproject_ltn_tool_metadata() {
        let toml = r#"
[project]
name = "h100-trainer"
version = "0.1.0"

[tool.ltn.resources]
node_count = 32
gpu_count = 256
gpu_model = "H100"
cpu_cores = 1024
memory_mb = 2097152
vram_mb = 20480000
workload_type = "PYTHON_VLLM_GPU"
"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_pyproject");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("pyproject.toml");
        std::fs::write(&file_path, toml).unwrap();

        let prof = ManifestParser::parse_pyproject_ltn_tool(&file_path).expect("Must parse pyproject tool.ltn");
        assert_eq!(prof.node_count, Some(32));
        assert_eq!(prof.gpu_count, Some(256));
        assert_eq!(prof.cpu_cores, Some(1024));
        assert_eq!(prof.memory_mb, Some(2097152));
        assert_eq!(prof.vram_mb, Some(20480000));
        assert_eq!(prof.gpu_model.as_deref(), Some("H100"));
        assert_eq!(prof.workload_type.as_deref(), Some("PYTHON_VLLM_GPU"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
