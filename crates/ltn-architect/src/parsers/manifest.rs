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
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub container_names: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub base_image: Option<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
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
        self.detected_files.iter().any(|f| {
            let fl = f.to_lowercase();
            fl == "docker-compose.yml"
                || fl == "docker-compose.yaml"
                || fl == "compose.yml"
                || fl == "compose.yaml"
                || (fl.contains("compose") && (fl.ends_with(".yml") || fl.ends_with(".yaml")))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.detected_files.is_empty()
    }

    pub fn services(&self) -> &[String] {
        self.structured_profile
            .as_ref()
            .map(|p| p.services.as_slice())
            .unwrap_or(&[])
    }

    pub fn ports(&self) -> &[String] {
        self.structured_profile
            .as_ref()
            .map(|p| p.ports.as_slice())
            .unwrap_or(&[])
    }

    pub fn container_names(&self) -> &[String] {
        self.structured_profile
            .as_ref()
            .map(|p| p.container_names.as_slice())
            .unwrap_or(&[])
    }

    pub fn base_image(&self) -> Option<&str> {
        self.structured_profile
            .as_ref()
            .and_then(|p| p.base_image.as_deref())
    }

    pub fn entrypoint(&self) -> Option<&str> {
        self.structured_profile
            .as_ref()
            .and_then(|p| p.entrypoint.as_deref())
    }
}

pub struct ManifestParser;

impl ManifestParser {
    pub fn merge_profiles(
        primary: &mut StructuredScaleProfile,
        companion: &StructuredScaleProfile,
    ) {
        if primary.workload_type.is_none() {
            primary.workload_type = companion.workload_type.clone();
        }
        if primary.node_count.is_none() {
            primary.node_count = companion.node_count;
        }
        if primary.gpu_count.is_none() {
            primary.gpu_count = companion.gpu_count;
        }
        if primary.gpu_model.is_none() {
            primary.gpu_model = companion.gpu_model.clone();
        }
        if primary.cpu_cores.is_none() {
            primary.cpu_cores = companion.cpu_cores;
        }
        if primary.memory_mb.is_none() {
            primary.memory_mb = companion.memory_mb;
        }
        if primary.vram_mb.is_none() {
            primary.vram_mb = companion.vram_mb;
        }
        if primary.base_image.is_none() {
            primary.base_image = companion.base_image.clone();
        }
        if primary.entrypoint.is_none() {
            primary.entrypoint = companion.entrypoint.clone();
        }
        for s in &companion.services {
            if !primary.services.contains(s) {
                primary.services.push(s.clone());
            }
        }
        for c in &companion.container_names {
            if !primary.container_names.contains(c) {
                primary.container_names.push(c.clone());
            }
        }
        for p in &companion.ports {
            if !primary.ports.contains(p) {
                primary.ports.push(p.clone());
            }
        }
    }

    pub fn parse_project_profile(path: &Path) -> Option<StructuredScaleProfile> {
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("project_profile.json")
        };
        let content = std::fs::read_to_string(&target).ok()?;
        let val: serde_json::Value = serde_json::from_str(&content).ok()?;

        let mut profile = StructuredScaleProfile::default();
        if let Some(wt) = val.get("workload_type").and_then(|v| v.as_str()) {
            profile.workload_type = Some(wt.to_string());
        }

        let demands = val.get("resource_demands").unwrap_or(&val);
        if profile.workload_type.is_none() {
            if let Some(wt) = demands.get("workload_type").and_then(|v| v.as_str()) {
                profile.workload_type = Some(wt.to_string());
            }
        }
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

        if let Some(services) = val
            .get("services")
            .or_else(|| demands.get("services"))
            .and_then(|v| v.as_array())
        {
            for s in services {
                if let Some(s_str) = s.as_str() {
                    if !profile.services.contains(&s_str.to_string()) {
                        profile.services.push(s_str.to_string());
                    }
                }
            }
        }
        if let Some(ports) = val
            .get("ports")
            .or_else(|| demands.get("ports"))
            .and_then(|v| v.as_array())
        {
            for p in ports {
                if let Some(p_str) = p.as_str() {
                    if !profile.ports.contains(&p_str.to_string()) {
                        profile.ports.push(p_str.to_string());
                    }
                } else if let Some(p_num) = p.as_u64() {
                    let s = p_num.to_string();
                    if !profile.ports.contains(&s) {
                        profile.ports.push(s);
                    }
                }
            }
        }
        if let Some(cnames) = val
            .get("container_names")
            .or_else(|| demands.get("container_names"))
            .and_then(|v| v.as_array())
        {
            for c in cnames {
                if let Some(c_str) = c.as_str() {
                    if !profile.container_names.contains(&c_str.to_string()) {
                        profile.container_names.push(c_str.to_string());
                    }
                }
            }
        }

        Some(profile)
    }

    pub fn parse_topology_json(path: &Path) -> Option<StructuredScaleProfile> {
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("topology.json")
        };
        let content = std::fs::read_to_string(&target).ok()?;
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
            if total_gpus > 0 {
                profile.gpu_count = Some(total_gpus);
            }
            if total_cores > 0 {
                profile.cpu_cores = Some(total_cores);
            }
            if total_ram > 0 {
                profile.memory_mb = Some(total_ram);
            }
            if total_vram > 0 {
                profile.vram_mb = Some(total_vram);
            }
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
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join("pyproject.toml")
        };
        let content = std::fs::read_to_string(&target).ok()?;
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
                        "node_count" => {
                            if let Ok(n) = v.parse::<u32>() {
                                profile.node_count = Some(n);
                                found_any = true;
                            }
                        }
                        "gpu_count" => {
                            if let Ok(n) = v.parse::<u32>() {
                                profile.gpu_count = Some(n);
                                found_any = true;
                            }
                        }
                        "gpu_model" => {
                            profile.gpu_model = Some(v.to_string());
                            found_any = true;
                        }
                        "cpu_cores" => {
                            if let Ok(n) = v.parse::<u32>() {
                                profile.cpu_cores = Some(n);
                                found_any = true;
                            }
                        }
                        "memory_mb" => {
                            if let Ok(n) = v.parse::<u32>() {
                                profile.memory_mb = Some(n);
                                found_any = true;
                            }
                        }
                        "vram_mb" => {
                            if let Ok(n) = v.parse::<u32>() {
                                profile.vram_mb = Some(n);
                                found_any = true;
                            }
                        }
                        "workload_type" => {
                            profile.workload_type = Some(v.to_string());
                            found_any = true;
                        }
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

    pub fn parse_package_json_ltn(pkg_val: &serde_json::Value) -> Option<StructuredScaleProfile> {
        let ltn_obj = pkg_val.get("ltn")?;
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
        if let Some(gm) = ltn_obj.get("gpu_model").and_then(|v| v.as_str()) {
            prof.gpu_model = Some(gm.to_string());
            found = true;
        }
        if let Some(vm) = ltn_obj.get("vram_mb").and_then(|v| v.as_u64()) {
            prof.vram_mb = Some(vm as u32);
            found = true;
        }
        if let Some(wt) = ltn_obj.get("workload_type").and_then(|v| v.as_str()) {
            prof.workload_type = Some(wt.to_string());
            found = true;
        }

        if found {
            Some(prof)
        } else {
            None
        }
    }

    fn parse_memory_str(val: &str) -> Option<u32> {
        let s = val
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_lowercase();
        if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
            let (num_str, unit) = s.split_at(idx);
            let num: f64 = num_str.trim().parse().ok()?;
            match unit.trim() {
                "t" | "tb" | "tib" => Some((num * 1024.0 * 1024.0) as u32),
                "g" | "gb" | "gib" => Some((num * 1024.0) as u32),
                "m" | "mb" | "mib" => Some(num as u32),
                "k" | "kb" | "kib" => Some((num / 1024.0) as u32),
                _ => None,
            }
        } else {
            let num: u32 = s.parse().ok()?;
            if num > 1048576 {
                Some(num / (1024 * 1024))
            } else {
                Some(num)
            }
        }
    }

    pub fn parse_docker_compose(path: &Path) -> Option<StructuredScaleProfile> {
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            let candidates = [
                "docker-compose.yml",
                "docker-compose.yaml",
                "compose.yml",
                "compose.yaml",
            ];
            let mut found = None;
            for c in &candidates {
                let cp = path.join(c);
                if cp.exists() {
                    found = Some(cp);
                    break;
                }
            }
            found?
        };

        let content = std::fs::read_to_string(&target).ok()?;
        let mut compose_services_count: u32 = 0;
        let mut compose_mem_mb: u32 = 0;
        let mut services: Vec<String> = Vec::new();
        let mut container_names: Vec<String> = Vec::new();
        let mut ports: Vec<String> = Vec::new();

        let mut in_services = false;
        let mut in_ports = false;
        let mut current_service_indent = 0;

        let known_property_keys = [
            "build",
            "image",
            "container_name",
            "ports",
            "environment",
            "volumes",
            "deploy",
            "networks",
            "restart",
            "command",
            "entrypoint",
            "labels",
            "depends_on",
            "expose",
            "healthcheck",
            "logging",
            "secrets",
            "configs",
            "extra_hosts",
            "sysctls",
            "ulimits",
            "devices",
            "cap_add",
            "cap_drop",
            "security_opt",
            "user",
            "working_dir",
            "mem_limit",
            "mem_reservation",
            "cpus",
            "cpu_shares",
            "read_only",
            "stdin_open",
            "tty",
            "privileged",
            "env_file",
            "links",
            "external_links",
            "profiles",
            "network_mode",
            "pid",
            "ipc",
            "stop_signal",
            "stop_grace_period",
            "tmpfs",
            "dns",
            "dns_search",
        ];

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Detect top-level sections (indent 0)
            if indent == 0 && trimmed.ends_with(':') {
                let section_name = trimmed.trim_end_matches(':').trim().to_lowercase();
                if section_name == "services" {
                    in_services = true;
                    in_ports = false;
                    continue;
                } else if ["version", "networks", "volumes", "configs", "secrets"]
                    .contains(&section_name.as_str())
                {
                    in_services = false;
                    in_ports = false;
                    continue;
                } else if !known_property_keys.contains(&section_name.as_str()) {
                    // Compose v1 root-level service definition (indent 0)
                    let svc_name = trimmed.trim_end_matches(':').trim().to_string();
                    if !svc_name.is_empty() && !services.contains(&svc_name) {
                        services.push(svc_name);
                        compose_services_count += 1;
                    }
                    in_services = true;
                    in_ports = false;
                    current_service_indent = 0;
                    continue;
                }
            }

            // Ports parsing
            if in_ports {
                if trimmed.starts_with('-') {
                    let port_val = trimmed.trim_start_matches('-').trim();
                    if port_val.starts_with("target:") || port_val.starts_with("published:") {
                        if let Some(val) = port_val.split(':').nth(1) {
                            let clean = val.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !clean.is_empty() && !ports.contains(&clean) {
                                ports.push(clean);
                            }
                        }
                    } else {
                        let clean = port_val.trim_matches('"').trim_matches('\'').to_string();
                        if !clean.is_empty() && !ports.contains(&clean) {
                            ports.push(clean);
                        }
                    }
                    continue;
                } else if trimmed.starts_with("target:") || trimmed.starts_with("published:") {
                    if let Some(val) = trimmed.split(':').nth(1) {
                        let clean = val.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !clean.is_empty() && !ports.contains(&clean) {
                            ports.push(clean);
                        }
                    }
                    continue;
                } else if indent <= current_service_indent + 2 {
                    in_ports = false;
                }
            }

            // Memory limit extraction
            if trimmed.starts_with("mem_limit:")
                || (trimmed.starts_with("memory:") && !trimmed.ends_with(':'))
            {
                let val_str = trimmed
                    .split_once(':')
                    .map_or("", |x| x.1)
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_lowercase();
                if let Some(mb) = Self::parse_memory_str(&val_str) {
                    compose_mem_mb = compose_mem_mb.max(mb);
                } else if trimmed.contains("8g") || trimmed.contains("8G") {
                    compose_mem_mb = compose_mem_mb.max(8192);
                } else if trimmed.contains("16g") || trimmed.contains("16G") {
                    compose_mem_mb = compose_mem_mb.max(16384);
                } else if trimmed.contains("32g") || trimmed.contains("32G") {
                    compose_mem_mb = compose_mem_mb.max(32768);
                } else if trimmed.contains("4g") || trimmed.contains("4G") {
                    compose_mem_mb = compose_mem_mb.max(4096);
                } else if trimmed.contains("2g") || trimmed.contains("2G") {
                    compose_mem_mb = compose_mem_mb.max(2048);
                }
            }

            // Container name extraction
            if trimmed.starts_with("container_name:") {
                let cname = trimmed
                    .split_once(':')
                    .map_or("", |x| x.1)
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !cname.is_empty() && !container_names.contains(&cname) {
                    container_names.push(cname);
                }
            }

            // Ports header extraction
            if trimmed.starts_with("ports:") {
                let after_colon = trimmed.split_once(':').map_or("", |x| x.1).trim();
                if after_colon.starts_with('[') && after_colon.ends_with(']') {
                    let inner = &after_colon[1..after_colon.len() - 1];
                    for item in inner.split(',') {
                        let clean = item.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !clean.is_empty() && !ports.contains(&clean) {
                            ports.push(clean);
                        }
                    }
                    in_ports = false;
                } else if !after_colon.is_empty() {
                    let clean = after_colon.trim_matches('"').trim_matches('\'').to_string();
                    if !clean.is_empty() && !ports.contains(&clean) {
                        ports.push(clean);
                    }
                    in_ports = false;
                } else {
                    in_ports = true;
                }
                continue;
            }

            // Service detection under services:
            if in_services && trimmed.ends_with(':') && !trimmed.starts_with('-') {
                let key = trimmed.trim_end_matches(':').trim();
                let key_lower = key.to_lowercase();
                if !known_property_keys.contains(&key_lower.as_str())
                    && indent > 0
                    && (current_service_indent == 0 || indent <= current_service_indent)
                {
                    current_service_indent = indent;
                    if !services.contains(&key.to_string()) {
                        services.push(key.to_string());
                        compose_services_count += 1;
                    }
                    in_ports = false;
                }
            }
        }

        let total_services = compose_services_count.max(services.len() as u32);
        if total_services > 0
            || compose_mem_mb > 0
            || !ports.is_empty()
            || !container_names.is_empty()
        {
            let mut profile = StructuredScaleProfile::default();
            if total_services > 0 {
                profile.node_count = Some(total_services);
            }
            if compose_mem_mb > 0 {
                profile.memory_mb = Some(compose_mem_mb);
            }
            profile.services = services;
            profile.container_names = container_names;
            profile.ports = ports;
            Some(profile)
        } else {
            None
        }
    }

    pub fn parse_dockerfile(path: &Path) -> Option<StructuredScaleProfile> {
        let target = if path.is_file() {
            path.to_path_buf()
        } else {
            let candidates = [
                "Dockerfile",
                "Dockerfile.prod",
                "Dockerfile.dev",
                "Dockerfile.release",
            ];
            let mut found = None;
            for c in &candidates {
                let cp = path.join(c);
                if cp.exists() {
                    found = Some(cp);
                    break;
                }
            }
            if found.is_none() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        let fname_lower = fname.to_lowercase();
                        if fname_lower.ends_with(".dockerfile")
                            || fname_lower.starts_with("dockerfile.")
                        {
                            found = Some(entry.path());
                            break;
                        }
                    }
                }
            }
            found?
        };

        let content = std::fs::read_to_string(&target).ok()?;
        let mut base_image: Option<String> = None;
        let mut ports: Vec<String> = Vec::new();
        let mut entrypoint: Option<String> = None;
        let mut cmd: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let lower = trimmed.to_lowercase();

            if lower.starts_with("from ") {
                let rest = trimmed[5..].trim();
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let mut img = None;
                for p in parts {
                    if p.starts_with("--") {
                        continue;
                    }
                    if p.eq_ignore_ascii_case("as") {
                        break;
                    }
                    img = Some(p.trim_matches('"').trim_matches('\'').to_string());
                    break;
                }
                if let Some(i) = img {
                    base_image = Some(i);
                }
            } else if lower.starts_with("expose ") {
                let rest = trimmed[7..].trim();
                for token in rest.split_whitespace() {
                    let token = token
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim_matches(',');
                    let clean = token.split('/').next().unwrap_or(token);
                    if !clean.is_empty() && !ports.contains(&clean.to_string()) {
                        ports.push(clean.to_string());
                    }
                }
            } else if lower.starts_with("entrypoint ") {
                let rest = trimmed[11..].trim();
                if rest.starts_with('[') && rest.ends_with(']') {
                    if let Ok(args) = serde_json::from_str::<Vec<String>>(rest) {
                        entrypoint = Some(args.join(" "));
                    } else {
                        let inner = rest[1..rest.len() - 1].trim();
                        let args: Vec<String> = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        entrypoint = Some(args.join(" "));
                    }
                } else {
                    entrypoint = Some(rest.trim_matches('"').trim_matches('\'').to_string());
                }
            } else if lower.starts_with("cmd ") {
                let rest = trimmed[4..].trim();
                if rest.starts_with('[') && rest.ends_with(']') {
                    if let Ok(args) = serde_json::from_str::<Vec<String>>(rest) {
                        cmd = Some(args.join(" "));
                    } else {
                        let inner = rest[1..rest.len() - 1].trim();
                        let args: Vec<String> = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        cmd = Some(args.join(" "));
                    }
                } else {
                    cmd = Some(rest.trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }

        let final_entrypoint = entrypoint.or(cmd);
        if base_image.is_some() || !ports.is_empty() || final_entrypoint.is_some() {
            Some(StructuredScaleProfile {
                base_image,
                ports,
                entrypoint: final_entrypoint,
                ..Default::default()
            })
        } else {
            None
        }
    }

    /// Discovers manifests at the target path, whether it points to a directory or a specific file.
    pub fn discover_target(path: &Path) -> ManifestDiscovery {
        if !path.exists() {
            return ManifestDiscovery::default();
        }

        if path.is_file() {
            return Self::discover_file(path);
        }

        if !path.is_dir() {
            return ManifestDiscovery::default();
        }

        Self::discover_dir_internal(path)
    }

    /// Discovers manifests in a directory or processes a single manifest file directly.
    pub fn discover_in_dir(path: &Path) -> ManifestDiscovery {
        Self::discover_target(path)
    }

    /// Processes a single manifest file directly and inspects its parent directory if accessible.
    pub fn discover_file(path: &Path) -> ManifestDiscovery {
        let mut discovery = ManifestDiscovery::default();
        let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
        let file_name_lower = file_name.to_lowercase();

        // 1. Identify and parse the specific file directly
        let is_docker_compose = file_name_lower == "docker-compose.yml"
            || file_name_lower == "docker-compose.yaml"
            || file_name_lower == "compose.yml"
            || file_name_lower == "compose.yaml"
            || (file_name_lower.contains("compose")
                && (file_name_lower.ends_with(".yml") || file_name_lower.ends_with(".yaml")));

        let is_dockerfile = file_name_lower == "dockerfile"
            || file_name_lower.starts_with("dockerfile.")
            || file_name_lower.ends_with(".dockerfile")
            || (file_name_lower.contains("dockerfile") && !file_name_lower.ends_with(".json"));

        if is_docker_compose {
            discovery.detected_files.push(file_name.to_string());
            if file_name != "docker-compose.yml"
                && !discovery
                    .detected_files
                    .contains(&"docker-compose.yml".to_string())
            {
                discovery
                    .detected_files
                    .push("docker-compose.yml".to_string());
            }
            if let Some(prof) = Self::parse_docker_compose(path) {
                discovery.structured_profile = Some(prof);
            }
        } else if is_dockerfile {
            discovery.has_dockerfile = true;
            discovery.detected_files.push(file_name.to_string());
            if file_name != "Dockerfile"
                && !discovery.detected_files.contains(&"Dockerfile".to_string())
            {
                discovery.detected_files.push("Dockerfile".to_string());
            }
            if let Some(prof) = Self::parse_dockerfile(path) {
                discovery.structured_profile = Some(prof);
            }
        } else if file_name_lower == "package.json" {
            if let Ok(pkg_val) = Self::parse_package_json(path) {
                discovery.has_package_json = true;
                discovery.detected_files.push("package.json".to_string());
                if let Some(prof) = Self::parse_package_json_ltn(&pkg_val) {
                    discovery.structured_profile = Some(prof);
                }
            }
        } else if file_name_lower == "project_profile.json"
            || (file_name_lower.ends_with(".json") && file_name_lower.contains("project_profile"))
        {
            if let Some(prof) = Self::parse_project_profile(path) {
                discovery.structured_profile = Some(prof);
                discovery.detected_files.push(file_name.to_string());
                if file_name != "project_profile.json"
                    && !discovery
                        .detected_files
                        .contains(&"project_profile.json".to_string())
                {
                    discovery
                        .detected_files
                        .push("project_profile.json".to_string());
                }
            }
        } else if file_name_lower == "topology.json"
            || (file_name_lower.ends_with(".json") && file_name_lower.contains("topology"))
        {
            if let Some(prof) = Self::parse_topology_json(path) {
                discovery.structured_profile = Some(prof);
                discovery.detected_files.push(file_name.to_string());
                if file_name != "topology.json"
                    && !discovery
                        .detected_files
                        .contains(&"topology.json".to_string())
                {
                    discovery.detected_files.push("topology.json".to_string());
                }
            }
        } else if file_name_lower.ends_with(".json") {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    let has_nodes_array = val.get("nodes").and_then(|v| v.as_array()).is_some();
                    let has_proj_structure =
                        val.get("resource_demands").is_some() || val.get("workload_type").is_some();

                    if has_nodes_array {
                        if let Some(prof) = Self::parse_topology_json(path) {
                            discovery.structured_profile = Some(prof);
                            discovery.detected_files.push(file_name.to_string());
                        }
                    } else if has_proj_structure {
                        if let Some(prof) = Self::parse_project_profile(path) {
                            discovery.structured_profile = Some(prof);
                            discovery.detected_files.push(file_name.to_string());
                        }
                    } else if let Some(prof) = Self::parse_package_json_ltn(&val) {
                        discovery.structured_profile = Some(prof);
                        discovery.detected_files.push(file_name.to_string());
                    } else {
                        discovery.detected_files.push(file_name.to_string());
                    }
                }
            }
        } else if file_name_lower == "pyproject.toml"
            || (file_name_lower.ends_with(".toml") && file_name_lower.contains("pyproject"))
        {
            discovery.has_pyproject_toml = true;
            discovery.detected_files.push(file_name.to_string());
            if file_name != "pyproject.toml"
                && !discovery
                    .detected_files
                    .contains(&"pyproject.toml".to_string())
            {
                discovery.detected_files.push("pyproject.toml".to_string());
            }
            if let Some(prof) = Self::parse_pyproject_ltn_tool(path) {
                discovery.structured_profile = Some(prof);
            }
        } else if file_name_lower == "requirements.txt" {
            discovery.has_pyproject_toml = true;
            discovery
                .detected_files
                .push("requirements.txt".to_string());
        } else if file_name_lower == "cargo.toml" {
            discovery.has_cargo_toml = true;
            discovery.detected_files.push(file_name.to_string());
            if file_name != "Cargo.toml"
                && !discovery.detected_files.contains(&"Cargo.toml".to_string())
            {
                discovery.detected_files.push("Cargo.toml".to_string());
            }
        } else if file_name_lower == "go.mod" {
            discovery.has_go_mod = true;
            discovery.detected_files.push(file_name.to_string());
        } else if file_name_lower == "pom.xml" {
            discovery.has_pom_xml = true;
            discovery.detected_files.push(file_name.to_string());
        }

        // 2. Also inspect the file's parent directory if accessible
        let parent_dir: Option<&Path> = match path.parent() {
            Some(p) if p.as_os_str().is_empty() => Some(Path::new(".")),
            Some(p) if p.exists() && p.is_dir() => Some(p),
            _ => None,
        };

        if let Some(parent) = parent_dir {
            let parent_discovery = Self::discover_dir_internal(parent);

            discovery.has_package_json |= parent_discovery.has_package_json;
            discovery.has_cargo_toml |= parent_discovery.has_cargo_toml;
            discovery.has_pyproject_toml |= parent_discovery.has_pyproject_toml;
            discovery.has_go_mod |= parent_discovery.has_go_mod;
            discovery.has_pom_xml |= parent_discovery.has_pom_xml;
            discovery.has_dockerfile |= parent_discovery.has_dockerfile;

            for f in parent_discovery.detected_files {
                if !discovery.detected_files.contains(&f) {
                    discovery.detected_files.push(f);
                }
            }

            if let Some(parent_prof) = parent_discovery.structured_profile {
                if let Some(ref mut prof) = discovery.structured_profile {
                    Self::merge_profiles(prof, &parent_prof);
                } else {
                    discovery.structured_profile = Some(parent_prof);
                }
            }
        }

        discovery
    }

    fn discover_dir_internal(path: &Path) -> ManifestDiscovery {
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
                if let Some(ref mut existing) = discovery.structured_profile {
                    Self::merge_profiles(existing, &prof);
                } else {
                    discovery.structured_profile = Some(prof);
                }
                detected_files.push("topology.json".to_string());
            }
        }

        let pkg_path = path.join("package.json");
        if pkg_path.exists() {
            if let Ok(pkg_val) = Self::parse_package_json(&pkg_path) {
                discovery.has_package_json = true;
                detected_files.push("package.json".to_string());
                if let Some(prof) = Self::parse_package_json_ltn(&pkg_val) {
                    if let Some(ref mut existing) = discovery.structured_profile {
                        Self::merge_profiles(existing, &prof);
                    } else {
                        discovery.structured_profile = Some(prof);
                    }
                }
            }
        }

        let pyproject_path = path.join("pyproject.toml");
        if pyproject_path.exists() {
            discovery.has_pyproject_toml = true;
            detected_files.push("pyproject.toml".to_string());
            if let Some(prof) = Self::parse_pyproject_ltn_tool(&pyproject_path) {
                if let Some(ref mut existing) = discovery.structured_profile {
                    Self::merge_profiles(existing, &prof);
                } else {
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

        // Dockerfile detection and parsing
        let dockerfile_path = path.join("Dockerfile");
        if dockerfile_path.exists() {
            discovery.has_dockerfile = true;
            detected_files.push("Dockerfile".to_string());
            if let Some(prof) = Self::parse_dockerfile(&dockerfile_path) {
                if let Some(ref mut existing) = discovery.structured_profile {
                    Self::merge_profiles(existing, &prof);
                } else {
                    discovery.structured_profile = Some(prof);
                }
            }
        } else if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_string();
                let fname_lower = fname.to_lowercase();
                if fname_lower.starts_with("dockerfile.") || fname_lower.ends_with(".dockerfile") {
                    discovery.has_dockerfile = true;
                    detected_files.push(fname);
                    if let Some(prof) = Self::parse_dockerfile(&entry.path()) {
                        if let Some(ref mut existing) = discovery.structured_profile {
                            Self::merge_profiles(existing, &prof);
                        } else {
                            discovery.structured_profile = Some(prof);
                        }
                    }
                    break;
                }
            }
        }

        for compose_file in [
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ] {
            let compose_path = path.join(compose_file);
            if compose_path.exists() {
                if !detected_files.contains(&compose_file.to_string())
                    && !detected_files.contains(&"docker-compose.yml".to_string())
                {
                    detected_files.push(compose_file.to_string());
                }
                if let Some(prof) = Self::parse_docker_compose(&compose_path) {
                    if let Some(ref mut existing) = discovery.structured_profile {
                        Self::merge_profiles(existing, &prof);
                    } else {
                        discovery.structured_profile = Some(prof);
                    }
                }
                break;
            }
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

        let prof = ManifestParser::parse_project_profile(&file_path)
            .expect("Must parse project_profile.json");
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

        let prof =
            ManifestParser::parse_topology_json(&file_path).expect("Must parse topology.json");
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

        let prof = ManifestParser::parse_pyproject_ltn_tool(&file_path)
            .expect("Must parse pyproject tool.ltn");
        assert_eq!(prof.node_count, Some(32));
        assert_eq!(prof.gpu_count, Some(256));
        assert_eq!(prof.cpu_cores, Some(1024));
        assert_eq!(prof.memory_mb, Some(2097152));
        assert_eq!(prof.vram_mb, Some(20480000));
        assert_eq!(prof.gpu_model.as_deref(), Some("H100"));
        assert_eq!(prof.workload_type.as_deref(), Some("PYTHON_VLLM_GPU"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_target_docker_compose_direct_file() {
        let compose_content = r#"
version: '3.8'
services:
  web:
    image: nginx:alpine
    container_name: web-server
    ports:
      - "80:80"
      - "443:443"
    mem_limit: 512m
  api:
    image: node:20
    container_name: api-backend
    ports:
      - "8080:8080"
    mem_limit: 2g
"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_direct_compose");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let compose_path = temp_dir.join("docker-compose.yml");
        std::fs::write(&compose_path, compose_content).unwrap();

        let discovery = ManifestParser::discover_target(&compose_path);
        assert!(discovery.has_docker_compose(), "Must detect docker compose");
        assert!(discovery
            .detected_files
            .contains(&"docker-compose.yml".to_string()));

        let prof = discovery
            .structured_profile
            .expect("Must have structured profile from compose");
        assert_eq!(prof.node_count, Some(2));
        assert_eq!(prof.memory_mb, Some(2048));
        assert!(prof.services.contains(&"web".to_string()));
        assert!(prof.services.contains(&"api".to_string()));
        assert!(prof.container_names.contains(&"web-server".to_string()));
        assert!(prof.container_names.contains(&"api-backend".to_string()));
        assert!(prof.ports.contains(&"80:80".to_string()));
        assert!(prof.ports.contains(&"443:443".to_string()));
        assert!(prof.ports.contains(&"8080:8080".to_string()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_target_project_profile_direct_file() {
        let json = r#"{
            "project_id": "DC-114",
            "workload_type": "PYTHON_VLLM_GPU",
            "resource_demands": {
                "node_count": 16,
                "gpu_count": 128,
                "gpu_model": "H100-SXM5-80GB",
                "cpu_cores": 512,
                "memory_mb": 1048576,
                "vram_mb": 10240000
            }
        }"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_direct_profile");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("project_profile.json");
        std::fs::write(&file_path, json).unwrap();

        let discovery = ManifestParser::discover_target(&file_path);
        assert!(discovery
            .detected_files
            .contains(&"project_profile.json".to_string()));

        let prof = discovery
            .structured_profile
            .expect("Must have structured profile from project_profile.json");
        assert_eq!(prof.workload_type.as_deref(), Some("PYTHON_VLLM_GPU"));
        assert_eq!(prof.node_count, Some(16));
        assert_eq!(prof.gpu_count, Some(128));
        assert_eq!(prof.gpu_model.as_deref(), Some("H100-SXM5-80GB"));
        assert_eq!(prof.cpu_cores, Some(512));
        assert_eq!(prof.memory_mb, Some(1048576));
        assert_eq!(prof.vram_mb, Some(10240000));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_target_dockerfile_direct_file() {
        let dockerfile_content = r#"
FROM python:3.11-slim AS runtime
WORKDIR /app
COPY . .
EXPOSE 8000 8080
ENTRYPOINT ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
"#;
        let temp_dir = std::env::temp_dir().join("ltn_test_direct_dockerfile");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("Dockerfile");
        std::fs::write(&file_path, dockerfile_content).unwrap();

        let discovery = ManifestParser::discover_target(&file_path);
        assert!(discovery.has_dockerfile, "Must detect Dockerfile");
        assert!(discovery.detected_files.contains(&"Dockerfile".to_string()));

        let prof = discovery
            .structured_profile
            .expect("Must have structured profile from Dockerfile");
        assert_eq!(prof.base_image.as_deref(), Some("python:3.11-slim"));
        assert!(prof.ports.contains(&"8000".to_string()));
        assert!(prof.ports.contains(&"8080".to_string()));
        assert_eq!(
            prof.entrypoint.as_deref(),
            Some("uvicorn main:app --host 0.0.0.0 --port 8000")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_target_direct_file_with_parent_companion_manifests() {
        let temp_dir = std::env::temp_dir().join("ltn_test_companions");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. Dockerfile
        let df_content = "FROM node:20-alpine\nEXPOSE 3000\nENTRYPOINT [\"node\", \"server.js\"]\n";
        std::fs::write(temp_dir.join("Dockerfile"), df_content).unwrap();

        // 2. docker-compose.yml
        let compose_content = "version: '3.8'\nservices:\n  app:\n    container_name: node-app\n    ports:\n      - \"3000:3000\"\n    mem_limit: 1g\n";
        std::fs::write(temp_dir.join("docker-compose.yml"), compose_content).unwrap();

        // 3. project_profile.json
        let proj_content = r#"{ "workload_type": "NODE_POSTGRES_API", "resource_demands": { "cpu_cores": 4, "memory_mb": 4096 } }"#;
        std::fs::write(temp_dir.join("project_profile.json"), proj_content).unwrap();

        // Direct targeting of Dockerfile
        let discovery = ManifestParser::discover_target(&temp_dir.join("Dockerfile"));
        assert!(discovery.has_dockerfile);
        assert!(discovery.has_docker_compose());
        assert!(discovery.detected_files.contains(&"Dockerfile".to_string()));
        assert!(discovery
            .detected_files
            .contains(&"docker-compose.yml".to_string()));
        assert!(discovery
            .detected_files
            .contains(&"project_profile.json".to_string()));

        let prof = discovery
            .structured_profile
            .expect("Must have merged profile");
        assert_eq!(prof.base_image.as_deref(), Some("node:20-alpine"));
        assert_eq!(prof.entrypoint.as_deref(), Some("node server.js"));
        assert_eq!(prof.workload_type.as_deref(), Some("NODE_POSTGRES_API"));
        assert!(prof.services.contains(&"app".to_string()));
        assert!(prof.container_names.contains(&"node-app".to_string()));
        assert!(prof.ports.contains(&"3000".to_string()));
        assert!(prof.ports.contains(&"3000:3000".to_string()));
        assert_eq!(prof.cpu_cores, Some(4));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discover_target_compose_yaml_and_custom_dockerfile() {
        let temp_dir = std::env::temp_dir().join("ltn_test_custom_names");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let compose_path = temp_dir.join("compose.yaml");
        std::fs::write(
            &compose_path,
            "services:\n  worker:\n    image: worker:latest\n",
        )
        .unwrap();

        let discovery_compose = ManifestParser::discover_target(&compose_path);
        assert!(discovery_compose.has_docker_compose());
        assert!(discovery_compose
            .detected_files
            .contains(&"compose.yaml".to_string()));

        let dockerfile_path = temp_dir.join("Dockerfile.prod");
        std::fs::write(&dockerfile_path, "FROM rust:1.75\nEXPOSE 9000\n").unwrap();

        let discovery_df = ManifestParser::discover_target(&dockerfile_path);
        assert!(discovery_df.has_dockerfile);
        assert!(discovery_df
            .detected_files
            .contains(&"Dockerfile.prod".to_string()));
        let prof = discovery_df.structured_profile.expect("Must have profile");
        assert_eq!(prof.base_image.as_deref(), Some("rust:1.75"));
        assert!(prof.ports.contains(&"9000".to_string()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
